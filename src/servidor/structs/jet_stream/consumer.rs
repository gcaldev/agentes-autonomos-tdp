use std::{
    io::Error,
    sync::mpsc::{self, Receiver, RecvError, RecvTimeoutError, Sender},
    thread,
    time::Duration,
};

use icwt_crypto::icwt_crypto::crypto::encrypt;
use serde_json::Value;

use crate::enums::errores_server::ErroresServer;

use super::file_processing::update_csv_file;

const RESOURCES_PATH: &str = "src/servidor/structs/jet_stream/resources/";
const CONSUMERS_PATH: &str = "consumers.csv";

#[derive(Debug)]
/// Implementacion que va ligada al PushConsumer, se ejecuta en un thread aparte, facilita el reenvio de mensajes y el acknoledge
struct ConcurrentConsumer {
    last_acknoledged: usize,
    js_receiver: Receiver<(String, usize)>,
    client_ack_recv: Receiver<usize>,
    notify: Sender<(String, usize)>,
    subject: String,
    contador: usize,
}

impl ConcurrentConsumer {
    /// Crea un nuevo ConcurrentConsumer
    pub fn new(
        js_receiver: Receiver<(String, usize)>,
        client_ack_recv: Receiver<usize>,
        notify: Sender<(String, usize)>,
        last_acknoledged: usize,
        subject: &str,
    ) -> ConcurrentConsumer {
        ConcurrentConsumer {
            js_receiver,
            client_ack_recv,
            notify,
            last_acknoledged,
            subject: subject.to_string(),
            contador: 0,
        }
    }

    /// Envia mensajes que no han sido procesados hasta que se cierra el canal de escritura
    fn process_messages(&mut self) {
        loop {
            println!("Esperando mensaje en consumer");
            if self.process_message().is_err() {
                break;
            }
            println!("Listo para leer siguiente mensaje");
        }
    }

    /// Reenvia mensajes luego de un timeout especificado
    pub fn process_messages_with_timeout(&mut self, msg: &str, id: usize) -> Result<(), RecvError> {
        match self.client_ack_recv.recv_timeout(Duration::from_millis(500)) {
            Ok(_) => Ok(()),
            Err(RecvTimeoutError::Timeout) => {
                println!("Timeout, volvemos a mandar el mensaje: {} id: {}", msg, id);

                if self.contador >= 5{
                    return Ok(());
                }

                self.send_msg(msg, id);
                self.process_messages_with_timeout(msg, id) // Continue processing messages with timeout
            }
            Err(RecvTimeoutError::Disconnected) => Err(RecvError),
        }
    }

    /// Envia un mensaje en caso de que no haya sido procesado
    fn process_message(&mut self) -> Result<(), RecvError> {
        let (msg, id) = self.js_receiver.recv()?;
        if self.last_acknoledged > id {
            println!(
                "Mensaje ya procesado, last_acknoledged: {}, id: {}",
                self.last_acknoledged, id
            );
            return Ok(());
        }
        println!("Procesando mensaje: {:?}, {}", msg, id);
        self.send_msg(&msg, id);
        self.process_messages_with_timeout(&msg, id)?;
        self.last_acknoledged = id;
        Ok(())
    }

    /// Realiza el envio de un mensaje
    fn send_msg(&mut self, msg: &str, id: usize) {
        let msg_with_ack_id = format!("{}.{}", id, msg);

        if let Ok(encrypted_data) = encrypt(msg_with_ack_id.as_bytes()) {
            let operation = format!(
                "PUB {} {}\r\n{}\r\n",
                self.subject,
                encrypted_data.len(),
                msg_with_ack_id
            );
            if self.notify.send((operation, 0)).is_err() {
                println!("Error al enviar la operacion, se pospone el envio");
            }
        }
        self.contador += 1;
    }
}

#[derive(Debug)]
/// Consumer de tipo push que escucha mensajes de un Jetstream y los envia al delivery subject
pub struct PushConsumer {
    _th_handler: Option<thread::JoinHandle<()>>,
    js_sender: Sender<(String, usize)>,
    client_ack_sender: Sender<usize>,
    pub name: String,
    pub delivery_subject: String,
    last_acknoledged: usize,
}

impl PushConsumer {
    /// Guarda la información del consumidor en un archivo CSV.
    pub fn to_csv(&self, filename: &str) -> Result<(), Error> {
        let updated_consumer_info = format!(
            "{},{},{}",
            self.name, self.delivery_subject, self.last_acknoledged
        );
        update_csv_file(filename, updated_consumer_info, self.name.to_string())
    }

    /// Inicializa un nuevo consumidor
    pub fn init(
        delivery_subject: &str,
        name: &str,
        notify: Sender<(String, usize)>,
        last_acknoledged: usize,
        filename: &str,
    ) -> Result<PushConsumer, ErroresServer> {
        let builder = thread::Builder::new().name(format!("consumer-{}", name));

        let (js_sender, js_receiver) = mpsc::channel::<(String, usize)>();
        let (client_ack_sender, client_ack_recv) = mpsc::channel::<usize>();

        let subject = delivery_subject.to_string();

        let _th_handler = Some(
            builder
                .spawn(move || {
                    let mut consumer = ConcurrentConsumer::new(
                        js_receiver,
                        client_ack_recv,
                        notify,
                        last_acknoledged,
                        &subject,
                    );
                    consumer.process_messages();
                })
                .map_err(|_| {
                    ErroresServer::InternalError("Error al crear el thread".to_string())
                })?,
        );

        let consumer = PushConsumer {
            js_sender,
            client_ack_sender,
            name: name.to_string(),
            _th_handler,
            delivery_subject: delivery_subject.to_string(),
            last_acknoledged,
        };
        let _ = consumer.to_csv(filename);

        Ok(consumer)
    }

    /// Crea un nuevo consumidor
    pub fn new(
        delivery_subject: &str,
        name: &str,
        notify: Sender<(String, usize)>,
        last_acknoledged: usize,
    ) -> Result<PushConsumer, ErroresServer> {
        PushConsumer::init(
            delivery_subject,
            name,
            notify,
            last_acknoledged,
            &(RESOURCES_PATH.to_owned() + CONSUMERS_PATH),
        )
    }

    /// Crea un consumidor a partir de un JSON
    pub fn from_json(
        json: &str,
        notify: Sender<(String, usize)>,
    ) -> Result<PushConsumer, ErroresServer> {
        let json: Value = serde_json::from_str(json).map_err(|_| ErroresServer::InvalidJson)?;
        println!("Json: {:?}", json);
        let delivery_subject = json["delivery_subject"]
            .as_str()
            .ok_or(ErroresServer::InvalidJson)?;
        let name = json["name"].as_str().ok_or(ErroresServer::InvalidJson)?;
        println!("Json subjects: {:?}", delivery_subject);

        PushConsumer::new(delivery_subject, name, notify, 0)
    }

    /// Envia un mensaje al subject de delivery
    pub fn send(&self, msg: &str, id: usize) -> Result<(), ErroresServer> {
        self.js_sender
            .send((msg.to_string(), id))
            .map_err(|_| ErroresServer::InternalError("Error al enviar mensaje".to_string()))?;
        Ok(())
    }

    /// Registra el acknoledge de un mensaje
    pub fn ack(&mut self, id: usize) -> Result<(), ErroresServer> {
        self.last_acknoledged = id;
        self.client_ack_sender
            .send(id)
            .map_err(|_| ErroresServer::InternalError("Error al enviar ack".to_string()))?;
        Ok(())
    }
}
