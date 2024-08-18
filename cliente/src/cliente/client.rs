use std::collections::HashMap;
use std::io;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Write;
use std::io::{BufReader, Read};
use std::str;
use std::sync::mpsc::{self};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;
use std::thread;
use std::thread::JoinHandle;

use super::iclient::INatsClient;
use super::suscriptor_handler::Consumer;
use super::suscriptor_handler::SuscriptionHandler;
use crate::cliente::errors::NatsClientError;
use crate::cliente::suscriptor_handler::SuscriptionInfo;
use crate::cliente::user::User;
use icwt_crypto::icwt_crypto::crypto::decrypt;
use icwt_crypto::icwt_crypto::crypto::decrypt_with_crlf;
use icwt_crypto::icwt_crypto::crypto::encrypt;
use icwt_crypto::icwt_crypto::crypto::encrypted_with_crlf;
use logger::logger::{LogType, Logger};

#[allow(dead_code)]
pub struct NatsClient<W: Write + Send, R: Read + Send> {
    writer: Arc<Mutex<W>>,
    reader: Arc<Mutex<R>>,
    writer_hashmap: Arc<Mutex<HashMap<String, W>>>,
    listener_handler: JoinHandle<()>,
    suscriptions: Arc<Mutex<Vec<SuscriptionHandler>>>,
    sub_id: usize,
    logger: Arc<Mutex<Logger>>,
    error_receiver: mpsc::Receiver<String>,
}

fn handle_subscription_update<W: Write + Send + 'static>(
    subscriptions: &Arc<Mutex<Vec<SuscriptionHandler>>>,
    subject: &str,
    reply_subject: Option<String>,
    mut payload: &str,
    hmsg: Option<&str>,
    listener_w: &Arc<Mutex<W>>,
) -> Result<(), NatsClientError> {
    let mut subs = subscriptions.lock().map_err(|_| {
        NatsClientError::InternalError("Error al tomar el lock de suscripciones".to_string())
    })?;

    let posible_sub = subs
        .iter()
        .position(|subscription| subscription.matches(subject));

    if let Some(index) = posible_sub {
        println!("Envia al suscriptor");
        // Lo removemos para obtener el ownership del struct
        let mut subscription: SuscriptionHandler = subs.remove(index);

        if let Some(ref consumer) = subscription.consumer {
            let writer = listener_w.lock().map_err(|_| {
                NatsClientError::InternalError(
                    "Error al intentar tomar el lock de escritura".to_string(),
                )
            })?;
            let mut iter = payload.split('.');
            if let Some(id) = iter.next() {
                payload = payload.trim_start_matches(&(id.to_string() + "."));
                write_msg(writer, &consumer.to_command(id))?;
            }
        }

        subscription
            .tx
            .send((
                payload.trim().to_string(),
                hmsg.map(|x| x.trim().to_string()),
                reply_subject,
            ))
            .map_err(|_| {
                NatsClientError::InternalError("Error al enviar un mensaje".to_string())
            })?;

        if let Some(max_msgs) = subscription.max_msgs {
            println!("Max msgs pendientes: {}", max_msgs);
            if max_msgs - 1 == 0 {
                println!("Unsubscribe luego de n mensajes");
                drop(subscription.tx);
                subscription.handler.join().map_err(|_| {
                    NatsClientError::InternalError(
                        "Error al unir el thread del suscriptor".to_string(),
                    )
                })?;
                return Ok(());
            }
            subscription.max_msgs = Some(max_msgs - 1);
        }
        subs.push(subscription);
        drop(subs);
    }
    Ok(())
}

fn parse_msg(msg: &str) -> Result<(String, usize, Option<String>), NatsClientError> {
    let splitted_msg: Vec<&str> = msg.split(' ').map(|x| x.trim()).collect();
    if splitted_msg.len() < 4 {
        return Err(NatsClientError::MalformedMessage);
    }
    let subject = splitted_msg[1].to_string();
    let buf_size_text;
    let reply_subject = if splitted_msg.len() == 4 {
        buf_size_text = splitted_msg[3];
        None
    } else {
        buf_size_text = splitted_msg[4];

        Some(splitted_msg[3].trim().to_string())
    };

    let buf_size = buf_size_text
        .parse()
        .map_err(|_| NatsClientError::MalformedMessage)?;

    Ok((subject, buf_size, reply_subject))
}

fn handle_msg<R: Read + Send + 'static, W: Write + Send + 'static>(
    received_msg: &str,
    nats_reader: &mut BufReader<&mut R>,
    subscriptions: &Arc<Mutex<Vec<SuscriptionHandler>>>,
    logger: &Arc<Mutex<Logger>>,
    listener_w: &Arc<Mutex<W>>,
) -> Result<(), NatsClientError> {
    //MSG <subject> <sid> [reply-to] <#bytes>␍␊[payload]␍␊
    match parse_msg(received_msg) {
        Ok((subject, buf_size, reply_subject)) => {
            println!("Subject parseado {} | {}", subject, received_msg);
            let mut payload = vec![0; buf_size];
            println!("Payload buf_size: {}", buf_size);
            println!("Payload size: {}", payload.len());

            match nats_reader.read_exact(&mut payload) {
                Ok(_) => {
                    println!("Payload en read {:?}", payload);
                    let payload = decrypt_with_crlf(&payload)
                        .map_err(|_| NatsClientError::MalformedMessage)?;

                    handle_subscription_update(
                        subscriptions,
                        &subject,
                        reply_subject,
                        &payload,
                        None,
                        listener_w,
                    )?;
                    logear(
                        logger,
                        LogType::Info,
                        &format!(
                            "Recibio mensaje: {} payload: {}",
                            received_msg,
                            payload.trim()
                        ),
                    );
                }
                Err(_) => {
                    logear(
                        logger,
                        LogType::Error,
                        &NatsClientError::InternalError("Error al leer el payload".to_string())
                            .to_string(),
                    );
                    return Err(NatsClientError::InternalError(
                        "Error al leer el payload".to_string(),
                    ));
                }
            }
        }
        Err(e) => {
            logear(
                logger,
                LogType::Error,
                &NatsClientError::InternalError("Error al tomar el subject".to_string())
                    .to_string(),
            );
            return Err(e);
        }
    }

    Ok(())
}

fn handle_hmsg<R: Read + Send + 'static, W: Write + Send + 'static>(
    received_msg: &str,
    nats_reader: &mut BufReader<&mut R>,
    subscriptions: &Arc<Mutex<Vec<SuscriptionHandler>>>,
    listener_w: &Arc<Mutex<W>>,
) -> Result<(), NatsClientError> {
    let splitted_msg: Vec<&str> = received_msg.split(' ').map(|x| x.trim()).collect();
    //HMSG <subject> <sid> [reply-to] <#header bytes> <#total bytes>␍␊[headers]␍␊␍␊[payload]␍␊
    match splitted_msg.get(1) {
        Some(subject) => {
            let mut buf_size_payload = 0;
            let buf_size_total;
            let mut buf_size_header = 0;

            let index_total;
            let index_header;
            let index_reply;

            let mut reply_subject = None;

            if splitted_msg.len() == 6 {
                index_total = 5;
                index_header = 4;
                index_reply = Some(3);
            } else {
                index_total = 4;
                index_header = 3;
                index_reply = None;
            }

            if let Some(total) = splitted_msg.get(index_total) {
                if let Some(header) = splitted_msg.get(index_header) {
                    buf_size_total = total.parse::<usize>().map_err(|_| {
                        NatsClientError::InternalError("Error al parsear string".to_string())
                    })?;
                    buf_size_header = header.parse::<usize>().map_err(|_| {
                        NatsClientError::InternalError("Error al parsear string".to_string())
                    })?;
                    buf_size_payload = buf_size_total - buf_size_header;

                    if let Some(index) = index_reply {
                        if let Some(reply) = splitted_msg.get(index) {
                            reply_subject = Some(reply);
                        }
                    }
                }
            }
            //que \r\n?
            // buf_size += 2; // para el \r\n

            println!("Subject parseado {} | {}", subject, received_msg);

            let mut headers = vec![0; buf_size_header];
            let mut payload = vec![0; buf_size_payload];

            println!("Header buf_size: {}", buf_size_header);
            println!("Header size: {}", headers.len());

            println!("Payload buf_size: {}", buf_size_payload);
            println!("Payload size: {}", payload.len());

            let header_str;
            let payload_str;
            match nats_reader.read_exact(&mut headers) {
                Ok(_) => {
                    println!("Headers en read exact {:?}", headers);
                    let decrypted_str = decrypt_with_crlf(&headers)
                        .map_err(|_| NatsClientError::MalformedMessage)?;

                    header_str = decrypted_str.trim().to_string();
                }
                Err(_) => {
                    return Err(NatsClientError::InternalError(
                        "Error al leer el payload".to_string(),
                    ))
                }
            }
            match nats_reader.read_exact(&mut payload) {
                Ok(_) => {
                    let decrypted_str = decrypt_with_crlf(&payload)
                        .map_err(|_| NatsClientError::MalformedMessage)?;

                    payload_str = decrypted_str.trim().to_string();
                }
                Err(_) => {
                    return Err(NatsClientError::InternalError(
                        "Error al leer el payload".to_string(),
                    ))
                }
            }
            handle_subscription_update(
                subscriptions,
                subject,
                reply_subject.map(|x| x.to_string()),
                &payload_str,
                Some(&header_str),
                listener_w,
            )?;
        }
        None => {
            return Err(NatsClientError::InternalError(
                "Error al tomar el subject".to_string(),
            ))
        }
    }
    Ok(())
}

fn write_msg<W: Write + Send + 'static>(
    mut stream: MutexGuard<W>,
    msg: &str,
) -> Result<(), NatsClientError> {
    let encrypted_data = encrypted_with_crlf(msg)
        .map_err(|_| NatsClientError::InternalError("Error al encriptar".to_string()))?;
    stream
        .write_all(&encrypted_data)
        .map_err(|_| NatsClientError::InternalError("Error al escribir el mensaje".to_string()))?;
    Ok(())
}

fn write_msg2<W: Write + Send + 'static>(stream: &mut W, msg: &str) -> Result<(), NatsClientError> {
    let encrypted_data = encrypted_with_crlf(msg)
        .map_err(|_| NatsClientError::InternalError("Error al encriptar".to_string()))?;
    stream
        .write_all(&encrypted_data)
        .map_err(|_| NatsClientError::InternalError("Error al escribir el mensaje".to_string()))?;
    Ok(())
}

fn read_until_crlf<R: Read>(reader: &mut R) -> io::Result<Vec<u8>> {
    let mut buffer = Vec::new();
    let mut temp_buffer = [0; 1];
    let mut last_byte = 0;

    loop {
        match reader.read_exact(&mut temp_buffer) {
            Ok(_) => {
                let byte = temp_buffer[0];
                if last_byte == b'\r' && byte == b'\n' {
                    buffer.pop();
                    break;
                }
                buffer.push(byte);
                last_byte = byte;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(buffer)
}

#[allow(clippy::significant_drop_in_scrutinee)]
fn process_incoming_msgs<W: Write + Send + 'static, R: Read + Send + 'static>(
    listener_w: Arc<Mutex<W>>,
    mut nats_reader: &mut BufReader<&mut R>,
    subscriptions: &Arc<Mutex<Vec<SuscriptionHandler>>>,
    logger: Arc<Mutex<Logger>>,
    error_notifier: mpsc::Sender<String>,
) -> Result<(), NatsClientError> {
    loop {
        match read_until_crlf(&mut nats_reader) {
            Ok(data) if data.is_empty() => {
                continue;
            }
            Ok(data) => {
                let decrpyted_msg = decrypt(&data).map_err(|_| {
                    NatsClientError::InternalError("Error al desencriptar el mensaje".to_string())
                })?;
                let received_msg = String::from_utf8_lossy(&decrpyted_msg);
                let splitted_msg: Vec<&str> = received_msg.split(' ').map(|x| x.trim()).collect();

                println!("MENSAJE split{:?}", splitted_msg);
                match splitted_msg.first() {
                    Some(operation) if operation.trim() == "PING" => {
                        println!("Recibe PING");
                        logear(&logger, LogType::Info, "Recibio PING");

                        match listener_w.lock() {
                            Ok(lock) => match write_msg(lock, "PONG\r\n") {
                                Ok(_) => {
                                    logear(&logger, LogType::Info, "Envio PONG");
                                    continue;
                                }
                                Err(e) => {
                                    println!("Error al enviar PONG: {}", e);
                                    logear(
                                        &logger,
                                        LogType::Warn,
                                        &format!("Error al enviar PONG: {}", e),
                                    );
                                }
                            },
                            Err(e) => {
                                println!("Error en Cliente: {}", e);
                                logear(
                                    &logger,
                                    LogType::Error,
                                    &format!("Error en Cliente: {}", e),
                                );
                                return Err(NatsClientError::InternalError(
                                    "Error al tomar el lock de escritura".to_string(),
                                ));
                            }
                        }
                    }
                    Some(operation) if operation.trim() == "+OK" => {
                        logear(&logger, LogType::Info, "Recibio +OK");
                        error_notifier.send("Ok".to_string()).map_err(|_| {
                            NatsClientError::InternalError(
                                "Error al enviar el mensaje de confirmacion".to_string(),
                            )
                        })?;
                        continue;
                    }
                    Some(operation) if operation.trim() == "MSG" => {
                        if let Err(e) = handle_msg(
                            &received_msg,
                            nats_reader,
                            subscriptions,
                            &logger,
                            &listener_w,
                        ) {
                            println!("Error en MSG: {}", e);
                            logear(&logger, LogType::Error, &e.to_string())
                        }
                        continue;
                    }
                    Some(operation) if operation.trim() == "HMSG" => {
                        let _ = handle_hmsg(&received_msg, nats_reader, subscriptions, &listener_w);
                        println!("Operation HMSG");
                        logear(&logger, LogType::Info, "Operation HMSG");
                    }
                    Some(operation) if operation.trim() == "-ERR" => {
                        println!("Error en la operacion: {:?}", received_msg);
                        logear(
                            &logger,
                            LogType::Warn,
                            &format!("Error en la operacion: {:?}", received_msg),
                        );
                        error_notifier.send(received_msg.to_string()).map_err(|_| {
                            NatsClientError::InternalError(
                                "Error al enviar el mensaje de error".to_string(),
                            )
                        })?;
                        break;
                    }
                    Some(_) => continue,
                    None => {}
                }
            }
            Err(e)
                if (e.kind() == ErrorKind::ConnectionAborted)
                    || (e.kind() == ErrorKind::UnexpectedEof) =>
            {
                println!("Se termino la comunicacion con el servidor");
                break;
            }
            Err(_) => {}
        }
        println!("continua")
    }
    Ok(())
}

#[allow(clippy::significant_drop_tightening)] //Por que al hacerle caso a clippy despues tira otro error y no hay forma de solucionarlo.
fn init_listener<W: Write + Send + 'static, R: Read + Send + 'static>(
    listener_r: Arc<Mutex<R>>,
    listener_w: Arc<Mutex<W>>,
    subscriptions: &Arc<Mutex<Vec<SuscriptionHandler>>>,
    logger: Arc<Mutex<Logger>>,
    error_notifier: mpsc::Sender<String>,
) -> Result<JoinHandle<()>, Error> {
    let builder = thread::Builder::new().name(String::from("listener"));

    let subscriptions = subscriptions.clone();
    let logger_th = logger.clone();

    let listener_handler = builder.spawn(move || {
        if let Ok(mut reader) = listener_r.lock() {
            let mut nats_reader: BufReader<&mut _> = BufReader::new(&mut *reader);
            if let Err(e) = process_incoming_msgs(
                listener_w,
                &mut nats_reader,
                &subscriptions,
                logger,
                error_notifier,
            ) {
                logear(&logger_th, LogType::Error, &e.to_string());
            }
        }
    })?;
    Ok(listener_handler)
}

fn logear(logger: &Arc<Mutex<Logger>>, tipo: LogType, msg: &str) {
    if let Ok(logger_lock) = logger.lock() {
        logger_lock.log(tipo, msg);
    } else {
        println!("Lock envenenado");
    }
}

impl<W: Write + Send + 'static, R: Read + Send + 'static> NatsClient<W, R> {
    pub fn new(
        mut writer: W,
        mut reader: R,
        log_file: &str,
        user: Option<User>,
    ) -> Result<Self, Error> {
        let logger = Arc::new(Mutex::new(Logger::new(log_file)));

        let raw_data = read_until_crlf(&mut reader).map_err(|_| {
            NatsClientError::InternalError("Error al leer el mensaje de confirmacion".to_string())
        })?;
        let decrypted_msg = decrypt(&raw_data).map_err(|_| {
            NatsClientError::InternalError("Error al desencriptar el mensaje".to_string())
        })?;
        let info_msg = String::from_utf8_lossy(&decrypted_msg);
        println!("{}", info_msg);

        let mut connect_msg = "CONNECT {".to_string();
        if let Some(user) = user {
            connect_msg.push_str(&format!(
                "\"user\":\"{}\",\"pass\":\"{}\"",
                user.user, user.pass
            ))
        }
        connect_msg.push_str("}\r\n");

        println!("Enviando: {}", connect_msg);
        write_msg2(&mut writer, &connect_msg)?;

        let (error_notifier, error_receiver) = mpsc::channel::<String>();

        let reader = Arc::new(Mutex::new(reader));
        let writer = Arc::new(Mutex::new(writer));

        let listener_r = reader.clone();
        let listener_w = writer.clone();

        let subs_info: Vec<SuscriptionHandler> = Vec::new();
        let suscriptions = Arc::new(Mutex::new(subs_info));

        let listener_handler = init_listener(
            listener_r,
            listener_w,
            &suscriptions,
            logger,
            error_notifier,
        )?;

        match error_receiver.recv() {
            Ok(msg) => {
                if msg.trim() == "Ok" {
                    return Ok(NatsClient {
                        writer,
                        reader,
                        suscriptions,
                        listener_handler,
                        sub_id: 0,
                        logger: Arc::new(Mutex::new(Logger::new(log_file))),
                        writer_hashmap: Arc::new(Mutex::new(HashMap::new())),
                        error_receiver,
                    });
                }
                Err(Error::new(std::io::ErrorKind::Other, msg))
            }
            Err(_) => Err(Error::new(
                std::io::ErrorKind::Other,
                "Error al recibir el mensaje de confirmacion",
            )),
        }
    }

    fn _subscribe(
        &mut self,
        subject: &str,
        f: Box<dyn Fn(SuscriptionInfo) + Send + 'static>,
        consumer: Option<Consumer>,
    ) -> Result<usize, NatsClientError> {
        if subject.is_empty() || subject.is_empty() {
            logear(
                &self.logger,
                LogType::Error,
                &NatsClientError::InvalidSubject.to_string(),
            );
            return Err(NatsClientError::InvalidSubject);
        }
        self.sub_id += 1;
        let sub_id = self.sub_id;
        let sub_msg = format!("SUB {} {}\r\n", subject, sub_id);
        println!("Enviando: {:?}", sub_msg);
        write_msg(self
            .writer
            .lock()
            .map_err(|e| NatsClientError::InternalError(format!("Error al hacer lock: {}", e)))?, &sub_msg)?;

        logear(
            &self.logger,
            LogType::Info,
            &format!("Enviando {}", sub_msg),
        );
        let sub_handler = SuscriptionHandler::new(sub_id, subject, f, consumer)?;

        self.suscriptions
            .lock()
            .map_err(|e| NatsClientError::InternalError(format!("Error al hacer lock: {}", e)))?
            .push(sub_handler);
        Ok(sub_id)
    }
}

fn calc_encrypted_len(data: &str) -> Result<usize, NatsClientError> {
    let mut result = 0;
    for text in data.split("\r\n") {
        result += 2;
        if text.is_empty() {
            continue;
        }
        result += encrypt(text.as_bytes())
            .map_err(|_| NatsClientError::InternalError("Error al encriptar".to_string()))?
            .len();
    }
    Ok(result)
}

impl<W: Write + Send + 'static, R: Read + Send + 'static> INatsClient for NatsClient<W, R> {
    fn create_stream(&mut self, subject: &str, name: &str) -> Result<(), NatsClientError> {
        let msg = format!(
            "$JS.API.STREAM.CREATE.{} {{ \"subjects\" : [\"{}\"], \"name\": \"{}\"}}\r\n",
            name, subject, name
        );
        println!("Enviando: |{:?}|", msg);

        logear(
            &self.logger,
            LogType::Info,
            &format!("Crear stream enviando: |{:?}|", msg),
        );

        write_msg(self
            .writer
            .lock()
            .map_err(|e| NatsClientError::InternalError(format!("Error al hacer lock: {}", e)))?, &msg)?;

        Ok(())
    }

    fn create_and_consume(
        &mut self,
        stream_name: &str,
        consumer_name: &str,
        delivery_subject: &str,
        f: Box<dyn Fn(SuscriptionInfo) + Send + 'static>,
    ) -> Result<usize, NatsClientError> {
        let msg = format!(
            "$JS.API.CONSUMER.CREATE.{} {{ \"delivery_subject\": \"{}\", \"name\": \"{}\"}}\r\n",
            stream_name, delivery_subject, consumer_name
        );
        println!("Enviando: |{:?}|", msg);

        logear(
            &self.logger,
            LogType::Info,
            &format!("Crear consumidor enviando: |{:?}|", msg),
        );

        write_msg(self
            .writer
            .lock()
            .map_err(|e| NatsClientError::InternalError(format!("Error al hacer lock: {}", e)))?, &msg)?;

        let consumer = Consumer::new(stream_name, consumer_name);

        self._subscribe(delivery_subject, f, Some(consumer))
    }

    fn publish(
        &mut self,
        subject: &str,
        payload: Option<&str>,
        reply_subject: Option<&str>,
    ) -> Result<(), NatsClientError> {
        let mut pub_msg = format!("PUB {} ", subject);

        if let Some(reply_subject) = reply_subject {
            pub_msg.push_str(reply_subject);
            pub_msg.push(' ');
        }

        let payload = payload.unwrap_or("");
        let enc_len = encrypt(payload.as_bytes())
            .map_err(|_| {
                NatsClientError::InternalError("Error al encriptar el mensaje".to_string())
            })?
            .len();
        pub_msg.push_str(&format!("{}\r\n{}\r\n", enc_len, payload));

        write_msg(self
            .writer
            .lock()
            .map_err(|e| NatsClientError::InternalError(format!("Error al hacer lock: {}", e)))?, &pub_msg)?;

        println!("Enviando: {:?}", pub_msg);
        logear(
            &self.logger,
            LogType::Info,
            &format!("Enviando: |{:?}|", pub_msg),
        );

        Ok(())
    }

    fn hpublish(
        &mut self,
        subject: &str,
        headers: &str,
        payload: &str,
        _reply_subject: Option<&str>,
    ) -> Result<(), Error> {
        let header_str = format!("{}\r\n\r\n", headers);
        let headers_len = calc_encrypted_len(&header_str)?;
        let payload_len = calc_encrypted_len(payload)?;
        let total_len = headers_len + payload_len;

        let mut pub_msg = format!("HPUB {} ", subject);
        println!("pub_msg: {:?}", pub_msg);
        // if let Some(reply) = reply_subject {
        //     pub_msg.push_str(reply);
        //     pub_msg.push_str(" ");
        // }

        pub_msg.push_str(&format!("{} {}{}", headers_len, total_len, "\r\n"));
        // println!("pub_msg 2: {:?}", pub_msg);

        pub_msg.push_str(&header_str);
        // println!("pub_msg 3: {:?}", pub_msg);

        pub_msg.push_str(payload);
        pub_msg.push_str("\r\n");

        let _ = self
            .writer
            .lock()
            .map_err(|e| NatsClientError::InternalError(format!("Error al hacer lock: {}", e)))?
            .write(pub_msg.as_bytes())
            .map_err(|e| {
                NatsClientError::InternalError(format!("Error al escribir en el stream: {}", e))
            })?;
        println!("Enviando: {:?}", pub_msg);
        logear(
            &self.logger,
            LogType::Info,
            &format!("Enviando: |{:?}|", pub_msg),
        );

        Ok(())
    }

    fn subscribe(
        &mut self,
        subject: &str,
        f: Box<dyn Fn(SuscriptionInfo) + Send + 'static>,
    ) -> Result<usize, NatsClientError> {
        self._subscribe(subject, f, None)
    }

    fn unsubscribe(
        &mut self,
        subject_id: usize,
        max_msgs: Option<usize>,
    ) -> Result<(), NatsClientError> {
        let mut unsub_msg = format!("UNSUB {}", subject_id); //\r\n

        if let Some(max_msgs) = max_msgs {
            unsub_msg.push(' ');
            unsub_msg.push_str(&max_msgs.to_string());
        }
        unsub_msg.push_str("\r\n");

        write_msg(self
            .writer
            .lock()
            .map_err(|e| NatsClientError::InternalError(format!("Error al hacer lock: {}", e)))?, &unsub_msg)?;

        println!("Enviando: {:?}", unsub_msg);
        logear(
            &self.logger,
            LogType::Info,
            &format!("Enviando {}", unsub_msg),
        );

        let mut subs = self
            .suscriptions
            .lock()
            .map_err(|e| NatsClientError::InternalError(format!("Error al hacer lock: {}", e)))?;

        let index = subs
            .iter()
            .position(|subscription| subscription.id == subject_id);

        if let Some(index) = index {
            if max_msgs.is_some() {
                let sub = subs.get_mut(index);
                if let Some(sub) = sub {
                    sub.max_msgs = max_msgs;
                    return Ok(());
                }
            }
            let sub = subs.remove(index);
            drop(subs);
            drop(sub.tx);
            if sub.handler.join().is_err() {
                return Err(NatsClientError::InternalError(
                    "Error al unir el thread del suscriptor".to_string(),
                ));
            }
            return Ok(());
        }
        logear(
            &self.logger,
            LogType::Error,
            &NatsClientError::SubscriptionNotFound.to_string(),
        );
        Err(NatsClientError::SubscriptionNotFound)
    }
}

#[cfg(test)]
mod tests_nats_client {

    use std::time::Duration;

    use icwt_crypto::icwt_crypto::crypto::decrypt_with_crlf;
    use serde_json::Value;
    use std::str;

    use super::*;
    /// MockTcpStream es una mock que implementa los traits Read y Write, los mismos que implementa el TcpStream
    struct MockTcpStream {
        write_data: Arc<Mutex<Vec<u8>>>,
        data_receiver: mpsc::Receiver<u8>,
    }

    impl Read for MockTcpStream {
        /// Lee bytes del stream hasta completar el buffer y devuelve cuantos bytes fueron leidos
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            for byte in buf.iter_mut() {
                *byte = 0;
            }

            let mut accumulated_bytes = Vec::new();
            let mut last_byte = 0;
            while accumulated_bytes.len() < buf.len() {
                match self.data_receiver.recv() {
                    Ok(byte) => {
                        accumulated_bytes.push(byte);
                        if last_byte == b'\r' && byte == b'\n' {
                            break;
                        }
                        last_byte = byte;
                    }
                    Err(_) => break,
                }
            }
            let len = accumulated_bytes.len();
            if len > 0 {
                buf[..len].copy_from_slice(&accumulated_bytes);
                Ok(len)
            } else {
                Ok(0)
            }
        }
    }

    impl Write for MockTcpStream {
        /// Escribe el valor del buffer en el stream y devuelve cuantos bytes fueron escritos
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut writer = self.write_data.lock().expect("Error al hacer lock");
            writer.write(buf)
        }

        fn flush(&mut self) -> std::io::Result<()> {
            let mut writer = self.write_data.lock().expect("Error al hacer lock");
            writer.flush()
        }
    }

    type MockCreationType = (Box<dyn Fn(&str)>, Arc<Mutex<Vec<u8>>>, MockTcpStream);

    fn init_mock_tcp_stream() -> MockCreationType {
        custom_mock_tcp_stream(Vec::new(), Vec::new())
    }

    fn mock_tcp_stream_with_connection() -> MockCreationType {
        custom_mock_tcp_stream(vec!["INFO {}\r\n", "+OK\r\n"], Vec::new())
    }

    fn clear(stream_data: &Arc<Mutex<Vec<u8>>>) {
        stream_data.clone().lock().unwrap().clear();
    }

    fn _calc_encrypted_len(data: &str) -> Result<usize, NatsClientError> {
        let mut result = 0;
        for text in data.split("\r\n") {
            result += 2;
            if text.is_empty() {
                continue;
            }
            result += encrypt(text.as_bytes())
                .map_err(|_| NatsClientError::InternalError("Error al encriptar".to_string()))?
                .len();
        }
        Ok(result)
    }

    fn custom_mock_tcp_stream(r: Vec<&str>, w: Vec<u8>) -> MockCreationType {
        let write_data = Arc::new(Mutex::new(w));
        let (tx, rx) = mpsc::channel::<u8>();

        let mock = MockTcpStream {
            write_data: write_data.clone(),
            data_receiver: rx,
        };

        let send_message = move |x: &str| {
            let encrypted = encrypted_from(x);
            for c in encrypted {
                tx.send(c).unwrap();
            }
        };

        r.iter().for_each(|x| send_message(x));

        (Box::new(send_message), write_data.clone(), mock)
    }

    fn parse_json(json: &str) -> (String, String) {
        let json: Value = serde_json::from_str(json).ok().unwrap();
        let user = json["user"].as_str().unwrap();
        let pass = json["pass"].as_str().unwrap();
        (user.to_string(), pass.to_string())
    }

    fn get_user_and_password_from(w_data: Arc<Mutex<Vec<u8>>>) -> (String, String) {
        let res = decrypt_with_crlf(&w_data.lock().unwrap().clone()).unwrap();
        let splitted_msg = res.split(" ").collect::<Vec<&str>>();
        parse_json(splitted_msg[1])
    }

    fn encrypted_from(msg: &str) -> Vec<u8> {
        let data = encrypted_with_crlf(msg).unwrap();
        data
    }

    #[test]
    fn test01_connect_ok() {
        let expected_msg = encrypted_from("CONNECT {}\r\n");
        let (_, write, writer_mock) = init_mock_tcp_stream();
        let (_, _, reader_mock) = mock_tcp_stream_with_connection();

        NatsClient::new(writer_mock, reader_mock, "logs.txt", None).unwrap();

        assert_eq!(write.lock().unwrap().clone(), expected_msg);
    }

    #[test]
    fn test02_ping_ok() {
        let expected_msg = encrypted_from("PONG\r\n");
        let (_, w_data, writer_mock) = init_mock_tcp_stream();
        let (send_data, _, reader_mock) =
            custom_mock_tcp_stream(vec!["INFO {}\r\n", "+OK\r\n"], Vec::new());
        NatsClient::new(writer_mock, reader_mock, "logs.txt", None).unwrap();
        clear(&w_data);

        send_data("PING\r\n");
        thread::sleep(Duration::from_secs(1));

        let res = w_data.lock().unwrap().clone();
        assert_eq!(res, expected_msg);
    }

    #[test]
    fn test03_publish_ok() {
        let enc_payload_len = encrypt("hola".as_bytes()).unwrap().len();
        let expected_msg =
            encrypted_from(&format!("PUB INCIDENTES {}\r\nhola\r\n", enc_payload_len));
        let (_, w_data, writer_mock) = init_mock_tcp_stream();
        let (_, _, reader_mock) = mock_tcp_stream_with_connection();
        let mut nats = NatsClient::new(writer_mock, reader_mock, "logs.txt", None).unwrap();
        clear(&w_data);

        nats.publish("INCIDENTES", Some("hola"), None).unwrap();

        let res = w_data.lock().unwrap().clone();
        assert_eq!(res, expected_msg);
    }

    #[test]
    fn test04_subscribe_ok() {
        let expected_sub_msg = "SUB INCIDENTES";
        let (_, w_data, writer_mock) = init_mock_tcp_stream();
        let (_, _, reader_mock) = mock_tcp_stream_with_connection();
        let mut nats = NatsClient::new(writer_mock, reader_mock, "logs.txt", None).unwrap();
        clear(&w_data);

        nats.subscribe(
            "INCIDENTES",
            Box::new(move |payload| {
                assert!(payload.0.trim() == "hola");
            }),
        )
        .unwrap();

        let binding = w_data.lock().unwrap().clone();
        let res = decrypt_with_crlf(&binding).unwrap();
        assert!(res.starts_with(expected_sub_msg) && res.ends_with("\r\n"));
    }

    #[test]
    fn test05_unsubscribe_ok() {
        let expected_unsub_msg = "UNSUB 1\r\n";
        let (_, w_data, writer_mock) = init_mock_tcp_stream();
        let (_, _, reader_mock) =
            custom_mock_tcp_stream(vec!["INFO {}\r\n", "+OK\r\n"], Vec::new());
        let mut nats = NatsClient::new(writer_mock, reader_mock, "logs.txt", None).unwrap();
        let sub_id = nats.subscribe("INCIDENTES", Box::new(move |_| {})).unwrap();
        clear(&w_data);

        nats.unsubscribe(sub_id, None).unwrap();

        let res = decrypt_with_crlf(&w_data.lock().unwrap().clone()).unwrap();
        assert_eq!(res, expected_unsub_msg);
    }

    #[test]
    fn test07_subscribe_empty_subject() {
        let (_, _w_data, writer_mock) = init_mock_tcp_stream();
        let (_, _, reader_mock) =
            custom_mock_tcp_stream(vec!["INFO {}\r\n", "+OK\r\n"], Vec::new());
        let mut client = NatsClient::new(writer_mock, reader_mock, "logs.txt", None).unwrap();

        // Prueba con un tema vacío
        let result = client.subscribe("", Box::new(|_| {}));

        assert!(matches!(result, Err(NatsClientError::InvalidSubject)));
    }

    #[test]
    fn test08_connect_with_login_ok() {
        let (_, w_data, writer_mock) = init_mock_tcp_stream();
        let (_, _, reader_mock) =
            custom_mock_tcp_stream(vec!["INFO {}\r\n", "+OK\r\n"], Vec::new());
        let expected_user = "user";
        let expected_pass = "pass";

        NatsClient::new(
            writer_mock,
            reader_mock,
            "logs.txt",
            Some(User::new(
                expected_user.to_string(),
                expected_pass.to_string(),
            )),
        )
        .unwrap();

        let (username, password) = get_user_and_password_from(w_data);
        assert_eq!(username, expected_user);
        assert_eq!(password, expected_pass);
    }

    #[test]
    fn test09_create_stream_ok() {
        let expected_msg = "$JS.API.STREAM.CREATE.js_incidentes { \"subjects\" : [\"INCIDENTES\"], \"name\": \"js_incidentes\"}\r\n";
        let (_, w_data, writer_mock) = init_mock_tcp_stream();
        let (_, _, reader_mock) = mock_tcp_stream_with_connection();
        let mut nats = NatsClient::new(writer_mock, reader_mock, "logs.txt", None).unwrap();
        clear(&w_data);

        nats.create_stream("INCIDENTES", "js_incidentes").unwrap();

        let res = decrypt_with_crlf(&w_data.lock().unwrap().clone()).unwrap();
        assert_eq!(res, expected_msg);
    }

    #[test]
    fn test10_create_consumer_ok() {
        let creation_command = "$JS.API.CONSUMER.CREATE.js_incidentes { \"delivery_subject\": \"incidentes_central_camara\", \"name\": \"central_camara_consumer\"}\r\n";
        let suscription_command = "SUB incidentes_central_camara 1\r\n";
        let expected_msg = format!("{}{}", creation_command, suscription_command);
        let (_, w_data, writer_mock) = init_mock_tcp_stream();
        let (_, _, reader_mock) = mock_tcp_stream_with_connection();
        let mut nats = NatsClient::new(writer_mock, reader_mock, "logs.txt", None).unwrap();
        clear(&w_data);

        nats.create_and_consume(
            "js_incidentes",
            "central_camara_consumer",
            "incidentes_central_camara",
            Box::new(move |_| {}),
        )
        .unwrap();

        let res = decrypt_with_crlf(&w_data.lock().unwrap().clone()).unwrap();
        assert_eq!(res, expected_msg);
    }
}
