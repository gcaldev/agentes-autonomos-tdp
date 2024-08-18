use super::suscription::Suscription;
use super::users_repository::UsersRepository;
use crate::enums::errores_server::ErroresServer;
use crate::structs::jet_stream::consumer::PushConsumer;
use crate::structs::jet_stream::js::JetStream;
use crate::structs::user_permissions::UserPermissions;
use icwt_crypto::icwt_crypto::crypto::encrypted_with_crlf;
use logger::logger::{LogType, Logger};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};
use std::io::{Error, Write};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, RwLock};

/// CommandExecutor ejecuta todas las operaciones recibidas por el servidor
/// para eso tiene channels que lo comunican con los oyentes de los clientes
/// los cuales le dan las operaciones a ejecutar. Tiene un mapa de clientes para gestionar
/// las conexiones de cada uno y poder hablarles a traves del TcpStream.
/// Conoce los permisos y credenciales a usar por los clientes para asi validarlos ("permissions", "auth_required" y "users_repository").
/// Y tiene un vector de suscriptores a los distintos topicos/subjects que existan.
pub struct CommandExecutor<W: Write + Send, R: UsersRepository> {
    clientes_map: Arc<Mutex<HashMap<usize, W>>>,
    clients_logs_map: Arc<RwLock<HashMap<usize, Logger>>>,
    permissions: HashMap<usize, UserPermissions>,
    js_streams: Vec<JetStream>,
    notification_receiver: Receiver<(String, usize)>,
    notification_sender: Sender<(String, usize)>,
    suscriptions: Vec<Suscription>,
    auth_required: bool,
    users_repository: R,
}

/// Escribe mensaje a traves del tcp stream
fn write_msg<W: Write + Send>(msg: &str, stream: &mut W) -> Result<(), ErroresServer> {
    let buf = encrypted_with_crlf(msg).map_err(|_| ErroresServer::ParserError)?;
    stream
        .write_all(&buf)
        .map_err(|_| ErroresServer::InternalError("Error al escribir el mensaje".to_string()))
}

impl<W: Write + Send, R: UsersRepository> CommandExecutor<W, R> {
    pub fn new(
        _: &str,
        notification_receiver: Receiver<(String, usize)>,
        notification_sender: Sender<(String, usize)>, // Para poder crear mas senders al crear consumers
        clientes_map: Arc<Mutex<HashMap<usize, W>>>,
        auth_required: bool,
        users_repository: R,
        clients_logs_map: Arc<RwLock<HashMap<usize, Logger>>>,
    ) -> Self {
        let noti = notification_sender.clone();
        CommandExecutor {
            notification_receiver,
            notification_sender,
            clientes_map,
            clients_logs_map,
            js_streams: JetStream::load_file_storage(noti).unwrap_or_default(),
            suscriptions: Vec::new(),
            permissions: HashMap::new(),
            auth_required,
            users_repository,
        }
    }

    /// Envia error a un cliente del servidor
    pub fn send_error(&self, error: String, client_id: usize) -> Result<(), ErroresServer> {
        println!("Enviando error al cliente");
        let mut streams = self
            .clientes_map
            .lock()
            .map_err(|_| ErroresServer::StreamsAccessError)?;
        let stream = streams
            .get_mut(&client_id)
            .ok_or(ErroresServer::StreamNotFound)?;
        write_msg(&format!("-ERR {}\r\n", error), stream)?;
        drop(streams);
        Ok(())
    }

    /// Parsea el mensaje de la conexion
    pub fn parse_connect(&self, mensaje: &str) -> Option<(String, String)> {
        let json: Value = serde_json::from_str(mensaje).ok()?;
        let user = json["user"].as_str()?;
        let pass = json["pass"].as_str()?;
        Some((user.to_string(), pass.to_string()))
    }

    /// Concecta a un cliente con el servidor, validando sus credenciales
    pub fn connect(&mut self, mensaje: &str, id: usize) -> Result<(), ErroresServer> {
        let mut streams = self
            .clientes_map
            .lock()
            .map_err(|_| ErroresServer::InternalError("Error al lockear".to_string()))?;
        let stream = streams.get_mut(&id).ok_or_else(|| {
            ErroresServer::InternalError("Error al acceder al stream del cliente".to_string())
        })?;
        println!("Connect mensaje: {} id: {}", mensaje, id);

        if !self.auth_required {
            return self.ok(stream);
        }

        if let Some((username, pass)) = self.parse_connect(mensaje) {
            println!("Parseo user: {:?} pass: {:?}", username, pass);
            if let Some(user) = self
                .users_repository
                .find_user_by_username_and_password(&username, &pass)
            {
                println!(
                    "Usuario encontrado: {} subjects permitidos: {:?}",
                    user.get_user(),
                    user.get_subscribe()
                );
                let permissions = UserPermissions::from(&user);
                self.permissions.insert(id, permissions);

                let mut log_lock = self
                    .clients_logs_map
                    .write()
                    .map_err(|_| ErroresServer::InternalError("Error al lockear".to_string()))?;
                (*log_lock).insert(id, Logger::new(user.get_log()));

                if let Some(logger) = (*log_lock).get_mut(&id) {
                    logger.log(LogType::Info, "CONNECT");
                }
                drop(log_lock);
                return self.ok(stream);
            }
        }

        write_msg(
            &format!("-ERR {}\r\n", ErroresServer::AuthorizationViolation),
            stream,
        )?;
        drop(streams);
        Err(ErroresServer::AuthorizationViolation)
    }

    /// Envia "+Ok" al cliente
    pub fn ok(&self, stream: &mut W) -> Result<(), ErroresServer> {
        write_msg("+OK\r\n", stream)?;
        println!("manda server +OK");
        Ok(())
    }

    /// Valida credenciales
    fn check_permissions<F: Fn(&UserPermissions) -> bool>(
        &self,
        connection_id: usize,
        additional_rule: F,
    ) -> bool {
        if self.auth_required {
            if let Some(permissions) = self.permissions.get(&connection_id) {
                return additional_rule(permissions);
            }
            return false;
        }
        true
    }

    /// Hace un log
    fn loggear(&self, id: &usize, error: LogType, error_msg: &str) {
        let logs_lock_result = self
            .clients_logs_map
            .write()
            .map_err(|_| ErroresServer::InternalError("Lock envenenado".to_string()));
        if let Ok(mut logs_lock) = logs_lock_result {
            if let Some(logger) = (*logs_lock).get_mut(id) {
                logger.log(error, error_msg);
            }
        }
    }

    /// Parsea el mesaje que le llega al SUB
    fn parse_sub(
        &mut self,
        mensaje: &str,
        connection_id: usize,
    ) -> Result<(String, String), ErroresServer> {
        println!("mensaje: |{}| connection_id: |{}|", mensaje, connection_id);
        let partes: Vec<&str> = mensaje.split_whitespace().collect();

        if partes.len() > 3 {
            let error = ErroresServer::InvalidSubject(
                "hay mas de 3 parametros en subject del SUB".to_string(),
            );
            let error_msg = error.to_string();
            self.loggear(&connection_id, LogType::Error, &error_msg);
            self.send_error(error_msg, connection_id)?;
            return Err(error);
        }

        if partes.len() <= 1 {
            let error = ErroresServer::InvalidSubject(
                "hay menos de 2 parametros en el subject del SUB".to_string(),
            );
            let error_msg = error.to_string();
            self.loggear(&connection_id, LogType::Error, &error_msg);
            self.send_error(error_msg, connection_id)?;
            return Err(error);
        }

        let id_cliente = partes[1];
        let subject = partes[0];

        if let Err(e) = validate_subject(partes[0]) {
            self.send_error(e.to_string(), connection_id)?;

            self.loggear(&connection_id, LogType::Error, e.to_string().as_str());
            return Err(e);
        }
        Ok((subject.to_string(), id_cliente.to_string()))
    }

    /// Suscribe a un cliente con un topico
    pub fn sub(&mut self, mensaje: &str, connection_id: usize) -> Result<(), ErroresServer> {
        let (subject, id_cliente) = self.parse_sub(mensaje, connection_id)?;

        let allowed_subject = self.check_permissions(connection_id, |permissions| {
            permissions.can_subscribe(&subject)
        });

        if !allowed_subject {
            return self.send_error(
                ErroresServer::SubscriptionPermissionsViolation(subject).to_string(),
                connection_id,
            );
        }

        let new_sub = Suscription::new(connection_id, subject.to_string(), None, id_cliente);
        self.suscriptions.push(new_sub);
        let mut streams = self
            .clientes_map
            .lock()
            .map_err(|_| ErroresServer::StreamsAccessError)?;
        let stream = streams.get_mut(&connection_id).ok_or_else(|| {
            ErroresServer::InternalError("Error al acceder al stream del cliente".to_string())
        })?;

        self.ok(stream)?;

        self.loggear(
            &connection_id,
            LogType::Info,
            format!("SUB {}", subject).as_str(),
        );
        drop(streams);
        Ok(())
    }

    /// Parse el mensaje que le llega al unsub
    fn parse_unsub(&self, mensaje: &str) -> Result<(String, Option<usize>), ErroresServer> {
        let partes: Vec<&str> = mensaje.split_whitespace().collect();
        match partes.len() {
            1 => Ok((partes[0].to_string(), None)),
            2 => {
                let sid = partes[0];
                let max_msgs = partes[1]
                    .parse()
                    .map_err(|_| ErroresServer::MalformedUnsubError)?;
                Ok((sid.to_string(), Some(max_msgs)))
            }
            _ => Err(ErroresServer::MalformedUnsubError),
        }
    }

    /// Desuscribe a un SID (id dado por el cliente) de un cierto subject. Lo hace de una o setea una cantidad maxima de mensajes
    pub fn unsub(&mut self, mensaje: &str, connection_id: usize) -> Result<(), ErroresServer> {
        println!("llega a UNSUB: {}", mensaje);

        let sid;
        let max_msgs;

        match self.parse_unsub(mensaje) {
            Ok(value) => {
                sid = value.0;
                max_msgs = value.1;
            }
            Err(e) => {
                self.send_error(
                    (ErroresServer::InvalidSubject(e.to_string())).to_string(),
                    connection_id,
                )?;

                self.loggear(&connection_id, LogType::Error, e.to_string().as_str());

                return Ok(());
            }
        }

        let i = self
            .suscriptions
            .iter()
            .position(|x| x.sid == sid && x.connection_id == connection_id);

        match i {
            Some(i) => {
                if let Some(max_msgs) = max_msgs {
                    self.suscriptions[i].max_msgs = Some(max_msgs);
                } else {
                    self.suscriptions.remove(i);
                }
            }
            None => {
                let error = ErroresServer::InvalidSubject(
                    "no hay coincidencias para el SID recibido".to_string(),
                )
                .to_string();
                self.loggear(&connection_id, LogType::Error, error.as_str());

                self.send_error(error, connection_id)?;

                return Ok(());
            }
        }

        let mut streams = self
            .clientes_map
            .lock()
            .map_err(|_| ErroresServer::StreamsAccessError)?;
        let stream = streams.get_mut(&connection_id).ok_or_else(|| {
            ErroresServer::InternalError("Error al acceder al stream del cliente".to_string())
        })?;

        self.ok(stream)
            .map_err(|_| ErroresServer::InternalError("Error al enviar ok".to_string()))?;
        if let Some(msg) = max_msgs {
            self.loggear(
                &connection_id,
                LogType::Info,
                format!("UNSUB: sid:{} max_msg:{}", sid, msg.to_string().as_str()).as_str(),
            );
        }
        drop(streams);
        Ok(())
    }

    /// Devuelve todos los suscriptores de determinado subject
    fn _get_subscriptions(&self, subject: &str) -> Result<Vec<&Suscription>, Error> {
        Ok(self
            .suscriptions
            .iter()
            .filter(|x| x.matches(subject))
            .collect())
    }

    /// Parsea el msg que le llega al publish
    fn parse_msg(
        &self,
        operation: &str,
        client_id: usize,
    ) -> Result<Option<(String, String, Option<String>)>, Error> {
        let primera_parte: Vec<&str> = operation.split("\r\n").collect();
        let separa_mensaje: Vec<&str> = primera_parte[0].split(' ').collect();

        //manejo primer mesnaje
        println!("operation: {}", operation);
        println!(
            "separa_mensaje: {:?}, len: {}",
            separa_mensaje,
            separa_mensaje.len()
        );
        match separa_mensaje.len() {
            2 => {
                let subject = separa_mensaje[0];
                let bytes = separa_mensaje[1];
                if bytes == "0" {
                    println!("no recibo payload");
                }
                Ok(Some((subject.to_string(), bytes.to_string(), None)))
            }
            3 => {
                let subject = separa_mensaje[0];
                let reply_to = separa_mensaje[1];
                let bytes = separa_mensaje[2];
                if bytes == "0" {
                    // Ejecutar comando
                }
                Ok(Some((
                    subject.to_string(),
                    bytes.to_string(),
                    Some(reply_to.to_string()),
                )))
            }
            largo => {
                let parametros = if largo <= 1 {
                    "tiene menos de dos parametros"
                } else {
                    "tiene mas de tres parametros"
                };
                self.send_error(
                    ErroresServer::InvalidSubject(format!(
                        "El subject {} ademas del PUB",
                        parametros
                    ))
                    .to_string(),
                    client_id,
                )?;

                Ok(None)
            }
        }
    }

    /// Quita las suscripciones a subjects que no le queden mensajes por recibir
    fn remove_subs_without_remaining_msgs(&mut self) {
        self.suscriptions.retain(|x| x.max_msgs != Some(0));
    }

    #[allow(clippy::type_complexity)]
    /// Parsea el mensaje para el hpublish
    fn parse_hmsg(
        &self,
        operation: &str,
        client_id: usize,
    ) -> Result<Option<(String, usize, usize, usize, Option<String>)>, ErroresServer> {
        //<subject> [reply-to] <#header bytes> <#total bytes>
        //            *
        let separa_mensaje: Vec<&str> = operation.split(' ').collect();

        //manejo primer mesnaje
        println!("operation: {}", operation);
        println!(
            "separa_mensaje: {:?}, len: {}",
            separa_mensaje,
            separa_mensaje.len()
        );

        //aca me llega todo hasta el primer "␍␊"
        match separa_mensaje.len() {
            3 => {
                //<subject>  <#header bytes> <#total bytes>
                let subject = separa_mensaje[0];
                let header_bytes = separa_mensaje[1];
                let total_bytes = separa_mensaje[2]; //The total size of headers and payload sections in bytes.

                let total_bytes_int = total_bytes.parse::<usize>().map_err(|_| {
                    ErroresServer::InternalError(
                        "Error al parsear el String total_bytes en hpublish".to_string(),
                    )
                })?;
                let header_bytes_int: usize = header_bytes.parse::<usize>().map_err(|_| {
                    ErroresServer::InternalError(
                        "Error al parsear el String headers_bytes en hpublish".to_string(),
                    )
                })?;

                let bytes_payload = total_bytes_int - header_bytes_int;

                return Ok(Some((
                    subject.to_string(),
                    header_bytes_int,
                    bytes_payload,
                    total_bytes_int,
                    None,
                )));
            }
            4 => {
                //<subject> <reply-to> <#header bytes> <#total bytes>
                let subject = separa_mensaje[0];
                let reply_to = separa_mensaje[1];
                let header_bytes = separa_mensaje[2];
                let total_bytes = separa_mensaje[3];

                let total_bytes_int = total_bytes.parse::<usize>().map_err(|_| {
                    ErroresServer::InternalError(
                        "Error al parsear el String total_bytes en hpublish".to_string(),
                    )
                })?;
                let header_bytes_int: usize = header_bytes.parse::<usize>().map_err(|_| {
                    ErroresServer::InternalError(
                        "Error al parsear el String headers_bytes en hpublish".to_string(),
                    )
                })?;

                let bytes_payload = total_bytes_int - header_bytes_int;

                return Ok(Some((
                    subject.to_string(),
                    header_bytes_int,
                    bytes_payload,
                    total_bytes_int,
                    Some(reply_to.to_string()),
                )));
            }
            largo => {
                if largo > 4 {
                    self.send_error(
                        ErroresServer::InvalidSubject("argumentos de mas (mas de 4)".to_string())
                            .to_string(),
                        client_id,
                    )?;
                } else {
                    self.send_error(
                        ErroresServer::InvalidSubject(
                            "argumentos de menos (menos de 3)".to_string(),
                        )
                        .to_string(),
                        client_id,
                    )?;
                }
            }
        }
        Ok(None)
    }

    /// Escribe el MSG que genera el "publish" a los cliente suscriptos en el subject en el cual se publica
    pub fn write_publish_msg(
        &mut self,
        payload: &str,
        reply_to: Option<String>,
        subject: &str,
        bytes: &str,
        is_consumer: bool,
        client_id: usize,
    ) -> Result<(), ErroresServer> {
        println!("Payload recibido en publish: {}", payload);

        self.js_streams
            .iter_mut()
            .filter(|x| x.matches(subject))
            .for_each(|x| x.store_msg(payload));

        let subscriptions = self
            .suscriptions
            .iter_mut()
            .filter(|x| x.matches(subject))
            .collect::<Vec<_>>();

        let mut streams = self
            .clientes_map
            .lock()
            .map_err(|_| ErroresServer::StreamsAccessError)?;

        let mut log_msgs = Vec::new();

        for sub in subscriptions {
            if let Some(stream) = streams.get_mut(&sub.connection_id) {
                if sub.max_msgs.is_some() {
                    sub.decrease_remaining_msgs();
                }

                let msg = if let Some(ref reply_to) = reply_to {
                    format!(
                        "MSG {} {} {} {}\r\n{}\r\n",
                        subject, sub.sid, reply_to, bytes, payload
                    )
                } else {
                    format!("MSG {} {} {}\r\n{}\r\n", subject, sub.sid, bytes, payload)
                };

                if write_msg(&msg, stream).is_ok() {
                    log_msgs.push(msg);
                }
            }
        }

        log_msgs.iter().for_each(|msg| {
            self.loggear(&client_id, LogType::Info, msg);
        });

        if !is_consumer {
            let publisher_stream = streams.get_mut(&client_id).ok_or_else(|| {
                ErroresServer::InternalError("Error al acceder al stream del cliente".to_string())
            })?;
            self.ok(publisher_stream)?;
        }
        drop(streams);
        self.remove_subs_without_remaining_msgs();
        Ok(())
    }

    /// Publica a los suscriptores del topico especificado por el cliente enviandoles el mensaje tambien dado por el cliente
    pub fn publish(&mut self, operation: &str, client_id: usize) -> Result<(), ErroresServer> {
        println!("operation: |{}|, client_id: {}", operation, client_id);
        if let Ok(Some(aux)) = self.parse_msg(operation, client_id) {
            let subject = aux.0;
            let bytes = aux.1;
            let reply_to = aux.2;

            let is_consumer = client_id == 0; // Las connection id son a partir de 1, si llega cero el publisher es un consumer

            let allowed_subject = is_consumer
                || self
                    .check_permissions(client_id, |permissions| permissions.can_publish(&subject));

            if !allowed_subject {
                let error = ErroresServer::PublishPermissionsViolation(subject).to_string();
                self.loggear(&client_id, LogType::Error, error.as_str());
                return self.send_error(error, client_id);
            }
            println!(
                "payload en publish: {}, is_consumer: {}, subject: {}",
                operation, is_consumer, subject
            );
            if bytes == "0" {
                return self.write_publish_msg(
                    "",
                    reply_to,
                    &subject,
                    &bytes,
                    is_consumer,
                    client_id,
                );
            } else {
                let primera_parte: Vec<&str> = operation.split("\r\n").collect();
                let payload = primera_parte[1];
                self.write_publish_msg(
                    payload,
                    reply_to,
                    &subject,
                    &bytes,
                    is_consumer,
                    client_id,
                )?;
            }
        }
        Ok(())
    }

    /// Envia un PONG como respuesta a un PING recibido por el cliente
    fn send_pong(&self, client_id: usize) -> Result<(), ErroresServer> {
        let mut streams = self
            .clientes_map
            .lock()
            .map_err(|_| ErroresServer::StreamsAccessError)?;
        let stream = streams
            .get_mut(&client_id)
            .ok_or(ErroresServer::StreamNotFound)?;
        write_msg("PONG\r\n", stream)?;
        drop(streams);

        self.loggear(&client_id, LogType::Info, "PONG");

        Ok(())
    }

    /// Envia PING al cliente para mantener viva la conexion
    fn send_ping(&self, client_id: usize) -> Result<(), ErroresServer> {
        let mut streams = self
            .clientes_map
            .lock()
            .map_err(|_| ErroresServer::StreamsAccessError)?;
        let stream = streams
            .get_mut(&client_id)
            .ok_or(ErroresServer::StreamNotFound)?;
        write_msg("PING\r\n", stream)?;
        drop(streams);

        self.loggear(&client_id, LogType::Info, "PING");

        Ok(())
    }

    /// Corta la conexion con un cliente de terminado por el id que recibe por parametro
    fn shutdown(&mut self, client_id: usize) -> Result<(), ErroresServer> {
        println!("Realizamos el shutdown en command executor");
        let mut streams = self
            .clientes_map
            .lock()
            .map_err(|_| ErroresServer::StreamsAccessError)?;
        let s = streams.remove(&client_id);
        self.suscriptions.retain(|x| x.connection_id != client_id);

        match s {
            Some(mut stream) => {
                write_msg("SHUTDOWN\r\n", &mut stream)?;
                self.loggear(&client_id, LogType::Info, "SHUTDOWN");
            }
            None => {
                println!("No se encontro el cliente para hacer el shutdown");
                self.loggear(
                    &client_id,
                    LogType::Warn,
                    "No se encontro el cliente para hacer el shutdown",
                );
            }
        }

        drop(streams);
        Ok(())
    }

    #[allow(unused_assignments)]
    /// Publica a los suscriptores del topico especificado por el cliente enviandoles el mensaje tambien dado por el cliente. Con la excepcion de que
    /// ademas se pueden enviar headers.
    pub fn hpublish(&mut self, operation: &str, client_id: usize) -> Result<(), ErroresServer> {
        if let Some(tupla) = self.parse_hmsg(operation, client_id)? {
            let subject = tupla.0;
            let bytes_header = tupla.1;
            let _ = tupla.2;
            let total_bytes = tupla.3;
            let reply_to = tupla.4;

            let mut payload = String::new();
            let mut header_vec = Vec::new();

            let allowed_subject =
                self.check_permissions(client_id, |permissions| permissions.can_publish(&subject));

            if !allowed_subject {
                self.loggear(
                    &client_id,
                    LogType::Error,
                    &ErroresServer::PublishPermissionsViolation(subject.clone()).to_string(),
                );
                return self.send_error(
                    ErroresServer::PublishPermissionsViolation(subject).to_string(),
                    client_id,
                );
            }

            let mut header = String::new();
            if bytes_header > 0 {
                //leo los /r/n esten vacios o no
                let mut parar = false;
                let mut acum = 0;
                while !parar {
                    if let Ok(msg) = self.notification_receiver.recv() {
                        acum += msg.0.len() + 2;

                        header_vec.push(msg.0);

                        if acum + 2 == bytes_header {
                            parar = true;
                        }
                    } else {
                        return Ok(());
                    }
                }
                let mut deque = VecDeque::from(header_vec);
                if let Some(first) = deque.pop_front() {
                    header += &first;
                }

                for elemento in deque {
                    header += &("\r\n".to_string() + &elemento)
                }
            } else if self.notification_receiver.recv().is_ok() {
                //el msg es vacio pero se hace esto para sacar el mensaje del canal
                header = "".to_string();
            } else {
                return Ok(());
            }

            if total_bytes > bytes_header {
                let _ = self.notification_receiver.recv(); //lee vacio x el
                if let Ok(msg) = self.notification_receiver.recv() {
                    payload = msg.0;
                } else {
                    return Ok(());
                }
            } else if self.notification_receiver.recv().is_ok() {
                //el msg es vacio pero se hace esto para sacar el mensaje del canal
                if self.notification_receiver.recv().is_ok() {
                    //el msg es vacio pero se hace esto para sacar el mensaje del canal
                    payload = "".to_string();
                } else {
                    return Ok(());
                }
            } else {
                return Ok(());
            }

            let mut vec_msg: Vec<String> = Vec::new();

            let mut subscriptions = self
                .suscriptions
                .iter_mut()
                .filter(|x| x.matches(&subject))
                .collect::<Vec<&mut Suscription>>();

            let mut streams = self
                .clientes_map
                .lock()
                .map_err(|_| ErroresServer::StreamsAccessError)?;
            for sub in subscriptions.iter_mut() {
                if let Some(stream) = streams.get_mut(&sub.connection_id) {
                    if sub.max_msgs.is_some() {
                        sub.decrease_remaining_msgs();
                    }

                    let msg = if let Some(ref reply_to) = reply_to {
                        format!(
                            "HMSG {} {} {} {} {}\r\n{}\r\n\r\n{}\r\n",
                            subject, sub.sid, reply_to, bytes_header, total_bytes, header, payload
                        )
                    } else {
                        format!(
                            "HMSG {} {} {} {}\r\n{}\r\n\r\n{}\r\n",
                            subject, sub.sid, bytes_header, total_bytes, header, payload
                        )
                    };

                    println!("Mensaje a enviar: {}", msg);

                    if write_msg(&msg, stream).is_ok() {
                        vec_msg.push(msg);
                    }
                }
            }

            for msg in vec_msg {
                self.loggear(&client_id, LogType::Info, &msg);
            }

            drop(streams);
            self.remove_subs_without_remaining_msgs();
        }

        Ok(())
    }

    /// Crea un Stream
    fn create_stream(&mut self, mensaje: &str, client_id: usize) -> Result<(), ErroresServer> {
        match JetStream::from_json(mensaje.trim()) {
            Ok(js) => {
                let already_exists = self.js_streams.iter().any(|x| x.name == js.name);
                if already_exists {
                    println!("Stream ya existe");
                    return Ok(());
                }
                let _ = js.store_jetstream();
                self.js_streams.push(js);
                println!("Stream creado");
            }
            Err(e) => self.send_error(e.to_string(), client_id)?,
        }
        Ok(())
    }

    /// Crea un Consumer
    fn create_consumer(
        &mut self,
        stream_name: &str,
        mensaje: &str,
        client_id: usize,
    ) -> Result<(), ErroresServer> {
        println!("Stream name: {}", stream_name);
        let stream = self.js_streams.iter_mut().find(|x| x.name == stream_name);
        if let Some(jet_stream) = stream {
            println!("Previo a agregar consumer");
            let consumer =
                PushConsumer::from_json(mensaje.trim(), self.notification_sender.clone())?;
            jet_stream.add_consumer(consumer);
            println!("Consumer agregado");
        } else {
            let error = ErroresServer::StreamNotFound.to_string();
            self.loggear(&client_id, LogType::Error, &error);
            self.send_error(error, client_id)?;
        }
        Ok(())
    }

    /// Parsea el acknowledgment del command executor
    fn parse_ack_cmd(&self, cmd: &str) -> Result<(String, String, usize), ErroresServer> {
        let mut iter = cmd.split('.');
        let stream_name = iter.next().ok_or(ErroresServer::ParserError)?;
        let consumer_name = iter.next().ok_or(ErroresServer::ParserError)?;
        let ack_id = iter
            .next()
            .ok_or(ErroresServer::ParserError)?
            .parse()
            .map_err(|_| ErroresServer::ParserError)?;
        Ok((stream_name.to_string(), consumer_name.to_string(), ack_id))
    }

    /// Lleva a cabo el ack sobre un consumer y ack_id
    fn ack(
        &mut self,
        js_operation: &str,
        _msg: &str,
        client_id: usize,
    ) -> Result<(), ErroresServer> {
        println!("JS operation: {}", js_operation);
        match self.parse_ack_cmd(js_operation) {
            Ok((stream_name, consumer_name, ack_id)) => {
                println!("|{}| |{}|", stream_name, consumer_name);
                let js_stream = self.js_streams.iter_mut().find(|x| x.name == stream_name);
                if let Some(js_stream) = js_stream {
                    if let Err(e) = js_stream.ack(&consumer_name, ack_id) {
                        self.send_error(e.to_string(), client_id)?;
                    }
                } else {
                    self.send_error(ErroresServer::StreamNotFound.to_string(), client_id)?;
                }
                Ok(())
            }
            Err(e) => self.send_error(e.to_string(), client_id),
        }
    }

    /// Ejecuta cada comando recibido por el servidor
    pub fn ejecutar_comando(&mut self) -> Result<(), ErroresServer> {
        loop {
            println!("esperando para ejecutar");
            match self.notification_receiver.recv() {
                Ok(msg) => {
                    println!("Mensaje entrante: {:?}", msg);

                    let mensaje_completo = msg.0;
                    let cliente_id = msg.1;
                    if let Ok((comando, mensaje)) =
                        separar_comando_y_resto_de_mensaje(mensaje_completo)
                    {
                        println!("Comando: |{}| Mensaje: |{}|", comando, mensaje);
                        match comando.as_str() {
                            "CONSUMER_PUBLISH" => {
                                println!("Entra a consumer_publish")
                            }
                            "CONNECT" | "connect" => self.connect(&mensaje, msg.1)?,
                            "PUB" | "pub" => self.publish(&mensaje, msg.1)?,
                            "HPUB" | "hpub" => self.hpublish(&mensaje, msg.1)?,
                            "SUB" | "sub" => self.sub(&mensaje, msg.1)?,
                            "UNSUB" | "unsub" => self.unsub(&mensaje, msg.1)?,
                            "PING" | "ping" => self.send_pong(msg.1)?,
                            "SEND_PING" => self.send_ping(msg.1)?,
                            "SHUTDOWN" => self.shutdown(msg.1)?,
                            "PONG" | "pong" => (),
                            "-ERR" | "-err" => {
                                self.send_error(mensaje, cliente_id)?;
                            }
                            op if op.starts_with("$JS.API.STREAM.CREATE.") => {
                                self.create_stream(&mensaje, msg.1)?
                            }
                            op if op.starts_with("$JS.API.CONSUMER.CREATE.") => {
                                let stream_name = op.trim_start_matches("$JS.API.CONSUMER.CREATE.");
                                self.create_consumer(stream_name, &mensaje, msg.1)?
                            }
                            op if op.starts_with("$JS.ACK.") => {
                                let operation = op.trim_start_matches("$JS.ACK.");
                                self.ack(operation, &mensaje, msg.1)?;
                            }
                            _ => {
                                println!("Error ultima rama");
                                self.send_error(
                                    ErroresServer::UnknownProtocolOperation.to_string(),
                                    cliente_id,
                                )?
                            }
                        }
                    } else {
                        println!("Error al manejar el mensaje.");
                    }
                }
                Err(_) => break,
            }
        }
        Ok(())
    }
}

///Asegura que subject cumple las condiciones de protocolo. Por lo que '*' puede estar en algun lado y sólo en último subtopic podría estar '>'
pub fn validate_subject(subject: &str) -> Result<(), ErroresServer> {
    // 1. Separar por puntos
    let tokens: Vec<&str> = subject.split('.').collect();
    // println!("deb {:?}",tokens.clone());

    // 2. cada separacion cumple condiciones de protocolo
    for (indice, token) in tokens.iter().enumerate() {
        // Verifica que el token no sea vacío y que contenga solo caracteres alfanuméricos
        if token.is_empty() {
            return Err(ErroresServer::InvalidSubject(
                "los tokens no pueden ser vacios".to_string(),
            ));
        }
        if token
            .chars()
            .all(|c| !c.is_ascii_alphanumeric() && c != '*' && c != '>')
        {
            return Err(ErroresServer::InvalidSubject(
                "los tokens solo pueden contener caracteres alfanumericos".to_string(),
            ));
        }
        if token == &">" && tokens.len() - 1 > indice {
            return Err(ErroresServer::InvalidSubject(
                "el wildcard \'>\' solo puede ser el ultimo token".to_string(),
            ));
        }
    }

    // 4. Verifica que los tokens de comodín * se utilicen solo como comodin, si sale tem1.*tema2 no es valid, solo acepta: tema1.*.tema2
    for token in &tokens {
        if token.contains('*') && token.len() > 1 {
            return Err(ErroresServer::InvalidSubject(
                "el wildcard \'*\' se usa solo, no puede ir acompañado, dentro del mismo token"
                    .to_string(),
            ));
        }
    }

    Ok(())
}

///funcion para separar comando del resto del mensaje. Al encontrar un espacio o tab.
pub fn separar_comando_y_resto_de_mensaje(
    mensaje_completo_en_bytes: String,
) -> Result<(String, String), Error> {
    let mut caracteres_de_comando = 0;

    let mut comando = String::new();
    for c in mensaje_completo_en_bytes.chars() {
        caracteres_de_comando += 1;
        if c == ' ' {
            break;
        }
        comando.push(c);
    }

    Ok((
        comando,
        mensaje_completo_en_bytes[caracteres_de_comando..].to_string(),
    ))
}

#[cfg(test)]
mod test_command_executor_comandos {
    use icwt_crypto::icwt_crypto::crypto::decrypt_with_crlf;
    use logger::logger::Logger;

    use crate::structs::user::User;
    use crate::structs::users_repository::UsersRepository;
    use std::str;
    use std::sync::mpsc::RecvTimeoutError;
    use std::sync::RwLock;
    use std::{
        collections::HashMap,
        io::{self, Read, Write},
        sync::{
            mpsc::{self, Sender},
            Arc, Mutex,
        },
        time::Duration,
    };

    use super::CommandExecutor;

    /// MockTcpStream es una mock que implementa los traits Read y Write, los mismos que implementa el TcpStream
    struct MockUsersRepository {}

    impl MockUsersRepository {
        fn new() -> Self {
            MockUsersRepository {}
        }
    }

    impl UsersRepository for MockUsersRepository {
        fn find_user_by_username_and_password(
            &self,
            username: &str,
            password: &str,
        ) -> Option<User> {
            if username == "admin" && password == "admin" {
                Some(User::new(
                    "admin".to_string(),
                    "admin".to_string(),
                    vec!["time.>".to_string()],
                    vec!["otro".to_string(), "time.>".to_string()],
                    "admin_log.txt",
                ))
            } else {
                None
            }
        }
    }

    struct MockTcpStream {
        read_data: Vec<u8>,
        write_data: Vec<u8>,
        sender: Sender<String>,
    }

    impl MockTcpStream {
        #[allow(dead_code)]
        fn new(sender: Sender<String>) -> Self {
            MockTcpStream {
                read_data: Vec::new(),
                write_data: Vec::new(),
                sender,
            }
        }
    }

    impl Read for MockTcpStream {
        /// Lee bytes del stream hasta completar el buffer y devuelve cuantos bytes fueron leidos
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.read_data.as_slice().read(buf)
        }
    }

    impl Write for MockTcpStream {
        /// Escribe el valor del buffer en el stream y devuelve cuantos bytes fueron escritos
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let msg = decrypt_with_crlf(buf).unwrap();
            self.sender.send(msg.to_owned()).unwrap();
            self.write_data.write(msg.as_bytes()).unwrap();
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.write_data.flush()
        }
    }

    struct BeforeEachSetup {
        cmd_notifier: mpsc::Sender<(String, usize)>,
        cmd_receiver: mpsc::Receiver<(String, usize)>,
        clientes_map: Arc<Mutex<HashMap<usize, MockTcpStream>>>,
        clients_logs_map: Arc<RwLock<HashMap<usize, Logger>>>,
        repo: MockUsersRepository,
        get_written_data_map: HashMap<usize, Box<dyn Fn() -> String>>,
    }

    fn before_each(clients: usize) -> BeforeEachSetup {
        let (cmd_notifier, cmd_receiver) = mpsc::channel::<(String, usize)>();
        let mut hashmap: HashMap<usize, MockTcpStream> = HashMap::new();
        let mut hashmap_funciones: HashMap<usize, Box<dyn Fn() -> String>> = HashMap::new();

        for i in 1..clients + 1 {
            let (sender, receiver) = mpsc::channel::<String>();
            let client_connection = MockTcpStream {
                read_data: Vec::new(),
                write_data: Vec::new(),
                sender,
            };

            hashmap.insert(i, client_connection);
            let get_written_data = move || -> String {
                let res = receiver.recv_timeout(Duration::from_millis(1000));
                match res {
                    Ok(msg) => msg,
                    Err(RecvTimeoutError::Timeout) => "No recibio nada todavia".to_string(),
                    Err(RecvTimeoutError::Disconnected) => {
                        "Se desconecto la otra punta".to_string()
                    }
                }
            };
            hashmap_funciones.insert(i, Box::new(get_written_data));
        }
        let clientes_map = Arc::new(Mutex::new(hashmap));

        let repo = MockUsersRepository::new();

        BeforeEachSetup {
            cmd_notifier,
            cmd_receiver,
            clientes_map,
            clients_logs_map: Arc::new(RwLock::new(HashMap::new())),
            repo,
            get_written_data_map: hashmap_funciones,
        }
    }

    #[test]
    fn test01_sub_lanza_error_cuando_hay_mas_de_tres_parametros_en_el_subject() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg = "time some_queue 12 aaaa";
        let _ = command_executor.sub(msg, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "-ERR Invalid subject, hay mas de 3 parametros en subject del SUB\r\n"
        )
    }

    #[test]
    fn test02_sub_lanza_error_cuando_hay_menos_de_dos_parametros_en_el_subject() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg = "time";
        let _ = command_executor.sub(msg, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "-ERR Invalid subject, hay menos de 2 parametros en el subject del SUB\r\n"
        )
    }

    #[test]
    fn test03_sub_envia_error_a_cliente_cuando_hay_token_vacio_en_el_subject() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg = "time..us 1";
        let _ = command_executor.sub(msg, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "-ERR Invalid subject, los tokens no pueden ser vacios\r\n"
        )
    }

    #[test]
    fn test04_sub_envia_error_a_cliente_cuando_hay_token_no_alfanumerico_en_el_subject() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg1 = "time.?.us 1";
        let _ = command_executor.sub(msg1, client_id);

        let msg2 = "time.{}.us 2";
        let _ = command_executor.sub(msg2, client_id);

        let msg3 = "time.#.us 3";
        let _ = command_executor.sub(msg3, client_id);

        let msg4 = "time.'#][€½].us 4";
        let _ = command_executor.sub(msg4, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        let afirmacion1 = (setup.get_written_data_map.get(&client_id).unwrap())().as_str()
            == "-ERR Invalid subject, los tokens solo pueden contener caracteres alfanumericos\r\n";
        let afirmacion2 = (setup.get_written_data_map.get(&client_id).unwrap())().as_str()
            == "-ERR Invalid subject, los tokens solo pueden contener caracteres alfanumericos\r\n";
        let afirmacion3 = (setup.get_written_data_map.get(&client_id).unwrap())().as_str()
            == "-ERR Invalid subject, los tokens solo pueden contener caracteres alfanumericos\r\n";
        let afirmacion4 = (setup.get_written_data_map.get(&client_id).unwrap())().as_str()
            == "-ERR Invalid subject, los tokens solo pueden contener caracteres alfanumericos\r\n";

        assert!(afirmacion1 && afirmacion2 && afirmacion3 && afirmacion4);
    }

    #[test]
    fn test05_sub_envia_error_a_cliente_cuando_wildcard_mayor_no_es_el_ultimo_token_del_subject() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg = "time.>.us 1";
        let _ = command_executor.sub(msg, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "-ERR Invalid subject, el wildcard \'>\' solo puede ser el ultimo token\r\n"
        )
    }

    #[test]
    fn test06_sub_envia_error_a_cliente_cuando_wildcard_asterisco_no_es_el_unico_en_el_token() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg = "time.*us 1";
        let _ = command_executor.sub(msg, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        assert_eq!((setup.get_written_data_map.get(&client_id).unwrap())().as_str(), "-ERR Invalid subject, el wildcard \'*\' se usa solo, no puede ir acompañado, dentro del mismo token\r\n")
    }

    #[test]
    fn test07_sub_funciona_sin_wildcards() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg = "time.us 1";
        let _ = command_executor.sub(msg, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        let devuelve_ok =
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str() == "+OK\r\n";
        if let Ok(suscripciones) = command_executor._get_subscriptions("time.us") {
            let coincide_connection_id = client_id == suscripciones[0].connection_id;
            let coincide_sid = "1" == suscripciones[0].sid;
            assert!(devuelve_ok && coincide_connection_id && coincide_sid);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test08_sub_funciona_con_wildcard_asterisco() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id: usize = 1;
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", 1)
            .unwrap();

        let msg = "time.*.us 1";
        let _ = command_executor.sub(msg, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        let devuelve_ok =
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str() == "+OK\r\n";
        if let Ok(suscripciones) = command_executor._get_subscriptions("time.*.us") {
            let coincide_connection_id = client_id == suscripciones[0].connection_id;
            let coincide_sid = "1" == suscripciones[0].sid;
            assert!(devuelve_ok && coincide_connection_id && coincide_sid);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test09_sub_funciona_con_wildcard_mayor() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg = "time.> 1";
        let _ = command_executor.sub(msg, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        let devuelve_ok =
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str() == "+OK\r\n";
        if let Ok(suscripciones) = command_executor._get_subscriptions("time.>.us") {
            let coincide_connection_id = client_id == suscripciones[0].connection_id;
            let coincide_sid = "1" == suscripciones[0].sid;
            assert!(devuelve_ok && coincide_connection_id && coincide_sid);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test10_publish_envia_error_a_cliente_cuando_hay_menos_de_2_parametros_en_el_subject() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg = "time.us";
        let _ = command_executor.publish(msg, 1);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "-ERR Invalid subject, El subject tiene menos de dos parametros ademas del PUB\r\n"
        )
    }

    #[test]
    fn test11_publish_envia_error_a_cliente_cuando_hay_mas_de_3_parametros_en_el_subject() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg = "time.us reply.camara 0 \"algo mas\"";
        let _ = command_executor.publish(msg, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "-ERR Invalid subject, El subject tiene mas de tres parametros ademas del PUB\r\n"
        )
    }

    #[test]
    fn test12_publish_envia_error_a_cliente_cuando_hay_mas_de_3_parametros_en_el_subject() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg_pub = "time.us reply.camara 0 \"algo mas\"";
        let _ = command_executor.publish(msg_pub, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "-ERR Invalid subject, El subject tiene mas de tres parametros ademas del PUB\r\n"
        )
    }

    #[test]
    fn test13_publish_en_un_topic_sin_subs_no_manda_msg() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg_pub = "time reply.here 4\r\nhola\r\n";
        let _ = command_executor.publish(msg_pub, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok
        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "No recibio nada todavia"
        )
    }

    #[test]
    fn test14_publish_sin_payload_envia_msg_correcto() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, 1);

        let msg_pub = "time.us 0\r\n\r\n";

        let _ = command_executor.publish(msg_pub, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok
        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok

        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "MSG time.us 1 0\r\n\r\n"
        )
    }

    #[test]
    fn test15_publish_sin_sub_en_su_topic_pero_si_en_otro_envia_msg_correcto() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg_sub = "time.ar 1";
        let _ = command_executor.sub(msg_sub, client_id);

        let msg_pub = "time.us 4\r\nhola\r\n";

        let _ = command_executor.publish(msg_pub, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok, de coneccion establecida.
        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok, del sub.
        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok, del payload recibido.

        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "No recibio nada todavia"
        )
    }

    #[test]
    fn test16_publish_con_subscriptor_a_su_topic_envia_msg_correcto_siendo_el_mismo_cliente_el_que_hace_sub_y_pub(
    ) {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg_sub = "time.ar 1";
        let _ = command_executor.sub(msg_sub, client_id);

        let msg_pub = "time.ar 4\r\nhola\r\n";

        let _ = command_executor.publish(msg_pub, client_id);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok, de coneccion establecida.
        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok, del sub.

        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "MSG time.ar 1 4\r\nhola\r\n"
        )
    }

    #[test]
    fn test17_publish_con_subscriptor_a_su_topic_envia_msg_correcto_siendo_distinto_el_cliente_que_hace_sub_y_pub(
    ) {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.ar 1";
        let _ = command_executor.sub(msg_sub, client_id2);

        let msg_pub = "time.ar 4\r\nhola\r\n";

        let _ = command_executor.publish(msg_pub, client_id1);

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok, de coneccion establecida.
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok, del sub.

        let afirmacion1 = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str()
            == "MSG time.ar 1 4\r\nhola\r\n";

        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok, de coneccion establecida.

        let afirmacion2 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n";

        assert!(afirmacion1 && afirmacion2);
    }

    #[test]
    fn test18_publish_con_varios_clientes_distintos_escuchando_en_el_mismo_topic() {
        let setup = before_each(4);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;
        let client_id3 = 3;
        let client_id4 = 4;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id3)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id4)
            .unwrap();

        let msg_sub = "time.ar 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_sub = "time.ar 2";
        let _ = command_executor.sub(msg_sub, client_id3);
        let msg_sub = "time.ar 3";
        let _ = command_executor.sub(msg_sub, client_id4);

        let msg_pub = "time.ar 4\r\nhola\r\n";

        let _ = command_executor.publish(msg_pub, client_id1);

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok, de coneccion establecida.
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok, del sub.
        let _ = (setup.get_written_data_map.get(&client_id3).unwrap())().as_str(); //saco el ok, de coneccion establecida.
        let _ = (setup.get_written_data_map.get(&client_id3).unwrap())().as_str(); //saco el ok, del sub.
        let _ = (setup.get_written_data_map.get(&client_id4).unwrap())().as_str(); //saco el ok, de coneccion establecida.
        let _ = (setup.get_written_data_map.get(&client_id4).unwrap())().as_str(); //saco el ok, del sub.

        let afirmacion1 = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str()
            == "MSG time.ar 1 4\r\nhola\r\n";
        let afirmacion2 = (setup.get_written_data_map.get(&client_id3).unwrap())().as_str()
            == "MSG time.ar 2 4\r\nhola\r\n";
        let afirmacion3 = (setup.get_written_data_map.get(&client_id4).unwrap())().as_str()
            == "MSG time.ar 3 4\r\nhola\r\n";

        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok, de coneccion establecida.

        let afirmacion4 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n";

        assert!(afirmacion1 && afirmacion2 && afirmacion3 && afirmacion4);
    }

    #[test]
    fn test19_publish_con_un_sub_de_wildcard_mayor_funciona_correctamente() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.ar.> 1";
        let _ = command_executor.sub(msg_sub, client_id2);

        let msg_pub1 = "time.ar.chaco 4\r\nhola\r\n";

        let _ = command_executor.publish(msg_pub1, client_id1);

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok, de conexion establecida.
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok, del sub.

        let afirmacion1 = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str()
            == "MSG time.ar.chaco 1 4\r\nhola\r\n";

        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok, de conexion establecida.

        let afirmacion2 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n";

        let msg_pub2 = "time.ar.santa_cruz 4\r\nhola\r\n";
        let _ = command_executor.publish(msg_pub2, client_id1);

        let afirmacion3 = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str()
            == "MSG time.ar.santa_cruz 1 4\r\nhola\r\n";

        let afirmacion4 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n";

        assert!(afirmacion1 && afirmacion2 && afirmacion3 && afirmacion4);
    }

    #[test]
    fn test20_publish_con_sub_de_wildcard_asterisco_funciona_bien() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.* 1";
        let _ = command_executor.sub(msg_sub, client_id2);

        let msg_pub1 = "time.ar 4\r\nhola\r\n";

        let _ = command_executor.publish(msg_pub1, client_id1);

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok, de conexion establecida.
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok, del sub.

        let afirmacion1 = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str()
            == "MSG time.ar 1 4\r\nhola\r\n";

        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok, de conexion establecida.

        let afirmacion2 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n";

        let msg_pub2 = "time.ch 4\r\nhola\r\n";

        let _ = command_executor.publish(msg_pub2, client_id1);

        let afirmacion3 = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str()
            == "MSG time.ch 1 4\r\nhola\r\n";

        let afirmacion4 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n";

        assert!(afirmacion1 && afirmacion2 && afirmacion3 && afirmacion4);
    }

    #[test]
    fn test21_publish_con_sub_de_wildcard_asterisco_y_simbolo_mayor_funciona_bien() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.*.> 1";
        let _ = command_executor.sub(msg_sub, client_id2);

        let msg_pub1 = "time.ar 14\r\nprimera prueba\r\n";

        let _ = command_executor.publish(msg_pub1, client_id1);

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok, de conexion establecida.
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok, del sub.

        let afirmacion1 = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str()
            == "MSG time.ar 1 14\r\nprimera prueba\r\n";

        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok, de conexion establecida.

        let afirmacion2 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n";

        let msg_pub2 = "time.ch.santiago.cerrillos 14\r\nsegunda prueba\r\n";

        let _ = command_executor.publish(msg_pub2, client_id1);

        let afirmacion3 = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str()
            == "MSG time.ch.santiago.cerrillos 1 14\r\nsegunda prueba\r\n";

        let afirmacion4 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n";

        assert!(afirmacion1 && afirmacion2 && afirmacion3 && afirmacion4);
    }

    #[test]
    fn test22_unsub_sin_parametros_lanza_error() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_unsub = "";
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str();
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str();

        assert_eq!(
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(),
            "-ERR Invalid subject, Formato incorrecto de comando de desuscripcion\r\n"
        )
    }

    #[test]
    fn test23_unsub_con_mas_de_dos_parametros_lanza_error() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_unsub = "1 3 cualquiercosa";
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str();
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str();

        assert_eq!(
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(),
            "-ERR Invalid subject, Formato incorrecto de comando de desuscripcion\r\n"
        )
    }

    #[test]
    fn test24_unsub_con_mas_de_dos_parametros_lanza_error() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_unsub = "2 3"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub

        assert_eq!(
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(),
            "-ERR Invalid subject, no hay coincidencias para el SID recibido\r\n"
        )
    }

    #[test]
    fn test25_unsub_se_hace_correctamente() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);

        let clientes_antes = command_executor.suscriptions.len();

        let msg_unsub = "1"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub

        let llego_ok_unsub =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";

        let clientes_despues = command_executor.suscriptions.len();
        let cliente_quitado = clientes_antes == 1 && clientes_despues == 0;

        assert!(llego_ok_unsub && cliente_quitado)
    }

    #[test]
    fn test26_unsub_cuando_hay_dos_sub_saca_al_correcto() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.> 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_sub = "time.*.capital 2";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_sub = "time.> 3";
        let _ = command_executor.sub(msg_sub, client_id1);
        let msg_sub = "time.*.capital 4";
        let _ = command_executor.sub(msg_sub, client_id1);

        let clientes_antes = command_executor.suscriptions.len();

        let msg_unsub = "1"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();
        let msg_unsub = "2"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub

        let llego_ok_unsub1 =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";
        let llego_ok_unsub2 =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";

        let clientes_despues = command_executor.suscriptions.len();
        let cliente_quitado = clientes_antes == 4 && clientes_despues == 2;

        let primera_sub_client1 = command_executor._get_subscriptions("time.>").unwrap();
        let segunda_sub_client1 = command_executor
            ._get_subscriptions("time.*.capital")
            .unwrap(); //ademas me da "time.>".

        let primera_sub_bien = primera_sub_client1.len() == 1
            && primera_sub_client1[0].connection_id == 1
            && primera_sub_client1[0].sid == "3".to_string();
        let segunda_sub_bien = segunda_sub_client1.len() == 2
            && segunda_sub_client1[1].connection_id == 1
            && segunda_sub_client1[1].sid == "4".to_string();

        assert!(
            llego_ok_unsub1
                && llego_ok_unsub2
                && cliente_quitado
                && primera_sub_bien
                && segunda_sub_bien
        );
    }

    #[test]
    fn test27_unsub_de_muchos_sid_del_mismo_cliente_se_hace_correctamente() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_sub = "time.ca 2";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_sub = "time.mx 3";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_sub = "time.rd 4";
        let _ = command_executor.sub(msg_sub, client_id2);

        let clientes_antes = command_executor.suscriptions.len();

        let msg_unsub = "1"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();
        let msg_unsub = "2"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();
        let msg_unsub = "3"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();
        let msg_unsub = "4"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub

        let llego_ok_unsub1 =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";
        let llego_ok_unsub2 =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";
        let llego_ok_unsub3 =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";
        let llego_ok_unsub4 =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";

        let clientes_despues = command_executor.suscriptions.len();
        let cliente_quitado = clientes_antes == 4 && clientes_despues == 0;

        assert!(
            llego_ok_unsub1
                && llego_ok_unsub2
                && llego_ok_unsub3
                && llego_ok_unsub4
                && cliente_quitado
        )
    }

    #[test]
    fn test28_unsub_de_muchos_sid_de_distintos_clientes_se_hace_correctamente() {
        let setup = before_each(5);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;
        let client_id3 = 3;
        let client_id4 = 4;
        let client_id5 = 5;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id3)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id4)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id5)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_sub = "time.ca 2";
        let _ = command_executor.sub(msg_sub, client_id3);
        let msg_sub = "time.mx 3";
        let _ = command_executor.sub(msg_sub, client_id4);
        let msg_sub = "time.rd 4";
        let _ = command_executor.sub(msg_sub, client_id5);

        let clientes_antes = command_executor.suscriptions.len();

        let msg_unsub = "1"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();
        let msg_unsub = "2"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id3).unwrap();
        let msg_unsub = "3"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id4).unwrap();
        let msg_unsub = "4"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id5).unwrap();

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub

        let _ = (setup.get_written_data_map.get(&client_id3).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id3).unwrap())().as_str(); //saca ok del sub

        let _ = (setup.get_written_data_map.get(&client_id4).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id4).unwrap())().as_str(); //saca ok del sub

        let _ = (setup.get_written_data_map.get(&client_id5).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id5).unwrap())().as_str(); //saca ok del sub

        let llego_ok_unsub1 =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";
        let llego_ok_unsub2 =
            (setup.get_written_data_map.get(&client_id3).unwrap())().as_str() == "+OK\r\n";
        let llego_ok_unsub3 =
            (setup.get_written_data_map.get(&client_id4).unwrap())().as_str() == "+OK\r\n";
        let llego_ok_unsub4 =
            (setup.get_written_data_map.get(&client_id5).unwrap())().as_str() == "+OK\r\n";

        let clientes_despues = command_executor.suscriptions.len();
        let cliente_quitado = clientes_antes == 4 && clientes_despues == 0;

        assert!(
            llego_ok_unsub1
                && llego_ok_unsub2
                && llego_ok_unsub3
                && llego_ok_unsub4
                && cliente_quitado
        )
    }

    #[test]
    fn test29_unsub_por_max_msg_se_hace_correctamente() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);

        let clientes_antes = command_executor.suscriptions.len();

        let msg_unsub = "1 2"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();

        let msg_pub = "time.us 4\r\nhola\r\n";
        let _ = command_executor.publish(&msg_pub, client_id1);
        let _ = command_executor.publish(&msg_pub, client_id1);

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub
        let llego_ok_unsub =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";

        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saca ok de connect
        let ok_pub1 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n"; //saca ok del pub
        let ok_pub2 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n"; //saca ok del pub

        let clientes_despues = command_executor.suscriptions.len();
        let cliente_quitado = clientes_antes == 1 && clientes_despues == 0;

        assert!(llego_ok_unsub && ok_pub1 && ok_pub2 && cliente_quitado)
    }

    #[test]
    fn test30_unsub_de_muchos_sid_de_distintos_clientes_se_hace_correctamente() {
        let setup = before_each(4);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;
        let client_id3 = 3;
        let client_id4 = 4;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id3)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id4)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_sub = "time.ca 2";
        let _ = command_executor.sub(msg_sub, client_id3);
        let msg_sub = "time.mx 3";
        let _ = command_executor.sub(msg_sub, client_id4);

        let clientes_antes = command_executor.suscriptions.len();

        let msg_unsub = "1 2"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();
        let msg_unsub = "2 2"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id3).unwrap();
        let msg_unsub = "3 2"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id4).unwrap();

        let msg_pub = "time.us 4\r\nhola\r\n";
        let _ = command_executor.publish(&msg_pub, client_id1);
        let _ = command_executor.publish(&msg_pub, client_id1);

        let msg_pub = "time.ca 4\r\nhola\r\n";
        let _ = command_executor.publish(&msg_pub, client_id1);
        let _ = command_executor.publish(&msg_pub, client_id1);

        let msg_pub = "time.mx 4\r\nhola\r\n";
        let _ = command_executor.publish(&msg_pub, client_id1);
        let _ = command_executor.publish(&msg_pub, client_id1);

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub

        let _ = (setup.get_written_data_map.get(&client_id3).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id3).unwrap())().as_str(); //saca ok del sub

        let _ = (setup.get_written_data_map.get(&client_id4).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id4).unwrap())().as_str(); //saca ok del sub

        let llego_ok_unsub1 =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";
        let llego_ok_unsub2 =
            (setup.get_written_data_map.get(&client_id3).unwrap())().as_str() == "+OK\r\n";
        let llego_ok_unsub3 =
            (setup.get_written_data_map.get(&client_id4).unwrap())().as_str() == "+OK\r\n";

        let clientes_despues = command_executor.suscriptions.len();
        let cliente_quitado = clientes_antes == 3 && clientes_despues == 0;

        assert!(llego_ok_unsub1 && llego_ok_unsub2 && llego_ok_unsub3 && cliente_quitado)
    }

    #[test]
    fn test31_unsub_por_max_msg_quita_al_sub_correcto_sin_quitar_a_otros_subs() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.> 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_sub = "time.*.capital 2";
        let _ = command_executor.sub(msg_sub, client_id2);

        let clientes_antes = command_executor.suscriptions.len();

        let msg_unsub = "1 2"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();
        let msg_unsub = "2 2"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();

        let msg_pub = "time.us 4\r\nhola\r\n";
        let _ = command_executor.publish(&msg_pub, client_id1);
        let _ = command_executor.publish(&msg_pub, client_id1);

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub
        let llego_ok_unsub =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";

        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saca ok de connect
        let ok_pub1 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n"; //saca ok del pub
        let ok_pub2 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n"; //saca ok del pub

        let clientes_despues = command_executor.suscriptions.len();
        let cliente_quitado = clientes_antes == 2 && clientes_despues == 1;

        let primer_sub = command_executor
            ._get_subscriptions("time.*.capital")
            .unwrap();
        let primer_sub_esta_bien = primer_sub[0].sid == "2"
            && primer_sub[0].max_msgs.unwrap() == 2
            && primer_sub[0].connection_id == 2;

        assert!(llego_ok_unsub && ok_pub1 && ok_pub2 && cliente_quitado && primer_sub_esta_bien);
    }

    #[test]
    fn test32_publish_despues_de_un_unsub_funciona_correctamente() {
        let setup = before_each(3);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;
        let client_id3 = 3;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id3)
            .unwrap();

        let msg_sub = "time.> 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        let msg_sub = "time.*.capital 2";
        let _ = command_executor.sub(msg_sub, client_id3);

        let clientes_antes = command_executor.suscriptions.len();

        let msg_unsub = "1"; //el primero es el SID y el segundo es el max_msgs
        let _ = command_executor.unsub(&msg_unsub, client_id2).unwrap();

        let msg_pub = "time.ar.capital reply.here 4\r\nhola\r\n";
        let _ = command_executor.publish(&msg_pub, client_id1);

        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saca ok del sub
        let llego_ok_unsub =
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str() == "+OK\r\n";
        let no_llega_pub_client2 = (setup.get_written_data_map.get(&client_id2).unwrap())()
            .as_str()
            == "No recibio nada todavia";

        let _ = (setup.get_written_data_map.get(&client_id3).unwrap())().as_str(); //saca ok de connect
        let _ = (setup.get_written_data_map.get(&client_id3).unwrap())().as_str(); //saca ok del sub
        let llego_pub_client3 = (setup.get_written_data_map.get(&client_id3).unwrap())().as_str()
            == "MSG time.ar.capital 2 reply.here 4\r\nhola\r\n";

        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saca ok de connect
        let ok_pub1 =
            (setup.get_written_data_map.get(&client_id1).unwrap())().as_str() == "+OK\r\n"; //saca ok del pub

        let clientes_despues = command_executor.suscriptions.len();
        let cliente_quitado = clientes_antes == 2 && clientes_despues == 1;

        let primer_sub = command_executor
            ._get_subscriptions("time.*.capital")
            .unwrap();
        let primer_sub_esta_bien = primer_sub[0].sid == "2" && primer_sub[0].connection_id == 3;

        assert!(
            llego_ok_unsub
                && ok_pub1
                && llego_pub_client3
                && no_llega_pub_client2
                && cliente_quitado
                && primer_sub_esta_bien
        );
    }

    #[test]
    fn test33_hpublish_envia_error_a_cliente_cuando_hay_menos_de_3_parametros_en_el_subject() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg = "time.us 10"; //SOLO 2 PARAMETROS
        let _ = command_executor.hpublish(msg, 1);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok
                                                                                  //  println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id).unwrap())().as_str());
        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "-ERR Invalid subject, argumentos de menos (menos de 3)\r\n"
        )
    }

    #[test]
    fn test34_hpublish_envia_error_a_cliente_cuando_hay_mas_de_4_parametros_en_el_subject() {
        let setup = before_each(1);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id = 1;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id)
            .unwrap();

        let msg = "time.us forecast 10 15 aaaa"; //MAS DE 4 PARAMETROS
        let _ = command_executor.hpublish(msg, 1);

        let _ = (setup.get_written_data_map.get(&client_id).unwrap())().as_str(); //saco el ok
                                                                                  //  println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id).unwrap())().as_str());
        assert_eq!(
            (setup.get_written_data_map.get(&client_id).unwrap())().as_str(),
            "-ERR Invalid subject, argumentos de mas (mas de 4)\r\n"
        )
    }

    #[test]
    fn test35_hpublish_con_reply_to_sin_headers_y_sin_payload_funciona() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id2).unwrap())().as_str());q
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok

        let msg = "time.us forecast 0 0";
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = command_executor.hpublish(msg, client_id1);
        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        assert_eq!(
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(),
            "HMSG time.us 1 forecast 0 0\r\n\r\n\r\n\r\n"
        )
    }

    #[test]
    fn test36_hpublish_sin_reply_to_sin_headers_y_sin_payload_funciona() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id2).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok

        let msg = "time.us 0 0";
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = command_executor.hpublish(msg, client_id1);
        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        assert_eq!(
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(),
            "HMSG time.us 1 0 0\r\n\r\n\r\n\r\n"
        )
    }

    #[test]
    fn test37_hpublish_con_reply_to_con_headers_y_sin_payload_funciona() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id2).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok

        let msg = "time.us forecast 16 16";
        let _ = setup
            .cmd_notifier
            .send(("BSAS: LLUVIA".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = command_executor.hpublish(msg, client_id1);

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        assert_eq!(
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(),
            "HMSG time.us 1 forecast 16 16\r\nBSAS: LLUVIA\r\n\r\n\r\n"
        )
    }

    #[test]
    fn test38_hpublish_con_reply_to_con_headers_y_con_payload_funciona() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id2).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok

        let msg = "time.us forecast 17 35";
        let _ = setup
            .cmd_notifier
            .send(("BSAS: SOLEADO".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = setup
            .cmd_notifier
            .send(("Al menos no llueve".to_string(), client_id1));
        let _ = command_executor.hpublish(msg, client_id1);

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        assert_eq!(
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(),
            "HMSG time.us 1 forecast 17 35\r\nBSAS: SOLEADO\r\n\r\nAl menos no llueve\r\n"
        )
    }

    #[test]
    fn test39_hpublish_sin_reply_to_sin_headers_y_con_payload_funciona() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id2).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok

        let msg = "time.us 0 24";
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1)); //Porque leemos hasta los "\r\n", este esta x el doble "\r\n" osea "\r\n\r\n".
        let _ = setup
            .cmd_notifier
            .send(("Espero que no llueva".to_string(), client_id1));
        let _ = command_executor.hpublish(msg, client_id1);

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        assert_eq!(
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(),
            "HMSG time.us 1 0 24\r\n\r\n\r\nEspero que no llueva\r\n"
        )
    }

    #[test]
    fn test40_hpublish_sin_reply_to_con_headers_y_con_payload_funciona() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id2).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok

        let msg = "time.us 19 39";
        let _ = setup
            .cmd_notifier
            .send(("Tiempo: Nublado".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1)); //Porque leemos hasta los "\r\n", este esta x el doble "\r\n" osea "\r\n\r\n".
        let _ = setup
            .cmd_notifier
            .send(("Espero que no llueva".to_string(), client_id1));
        let _ = command_executor.hpublish(msg, client_id1);

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        assert_eq!(
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(),
            "HMSG time.us 1 19 39\r\nTiempo: Nublado\r\n\r\nEspero que no llueva\r\n"
        )
    }

    #[test]
    fn test41_hpublish_sin_reply_to_con_headers_y_sin_payload_funciona() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id2).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok

        let msg = "time.us 19 19";
        let _ = setup
            .cmd_notifier
            .send(("Tiempo: Nublado".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1)); //Porque leemos hasta los "\r\n", este esta x el doble "\r\n" osea "\r\n\r\n".
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = command_executor.hpublish(msg, client_id1);

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        assert_eq!(
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(),
            "HMSG time.us 1 19 19\r\nTiempo: Nublado\r\n\r\n\r\n"
        )
    }

    #[test]
    fn test42_hpublish_con_reply_to_sin_headers_y_con_payload_funciona() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id2).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok

        let msg = "time.us forecast 0 24";
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1)); //Porque leemos hasta los "\r\n", este esta x el doble "\r\n" osea "\r\n\r\n".
        let _ = setup
            .cmd_notifier
            .send(("Espero que no llueva".to_string(), client_id1));
        let _ = command_executor.hpublish(msg, client_id1);

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        assert_eq!(
            (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(),
            "HMSG time.us 1 forecast 0 24\r\n\r\n\r\nEspero que no llueva\r\n"
        )
    }

    #[test]
    fn test43_hpublish_con_header_grande_funciona() {
        let setup = before_each(2);
        let mut command_executor = CommandExecutor::new(
            "",
            setup.cmd_receiver,
            setup.cmd_notifier.clone(),
            setup.clientes_map,
            true,
            setup.repo,
            setup.clients_logs_map,
        );

        let client_id1 = 1;
        let client_id2 = 2;

        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id1)
            .unwrap();
        command_executor
            .connect(&"{\"user\":\"admin\",\"pass\":\"admin\"}", client_id2)
            .unwrap();

        let msg_sub = "time.us 1";
        let _ = command_executor.sub(msg_sub, client_id2);
        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id2).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok
        let _ = (setup.get_written_data_map.get(&client_id2).unwrap())().as_str(); //saco el ok

        let msg = "time.us forecast 89 111";
        let _ = setup
            .cmd_notifier
            .send(("Lunes: Soleado".to_string(), client_id1));
        let _ = setup
            .cmd_notifier
            .send(("Martes: Nublado".to_string(), client_id1));
        let _ = setup
            .cmd_notifier
            .send(("Miercoles: Soleado".to_string(), client_id1));
        let _ = setup
            .cmd_notifier
            .send(("Jueves: Nublado".to_string(), client_id1));
        let _ = setup
            .cmd_notifier
            .send(("Viernes: Lluvia".to_string(), client_id1));
        let _ = setup.cmd_notifier.send(("".to_string(), client_id1)); //Porque leemos hasta los "\r\n", este esta x el doble "\r\n" osea "\r\n\r\n".
        let _ = setup
            .cmd_notifier
            .send(("Espero que no llueva".to_string(), client_id1));
        let _ = command_executor.hpublish(msg, client_id1);

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        let _ = (setup.get_written_data_map.get(&client_id1).unwrap())().as_str(); //saco el ok

        //println!("El mensaje: {}", (setup.get_written_data_map.get(&client_id1).unwrap())().as_str());
        assert_eq!((setup.get_written_data_map.get(&client_id2).unwrap())().as_str(), "HMSG time.us 1 forecast 89 111\r\nLunes: Soleado\r\nMartes: Nublado\r\nMiercoles: Soleado\r\nJueves: Nublado\r\nViernes: Lluvia\r\n\r\nEspero que no llueva\r\n")
    }
}
