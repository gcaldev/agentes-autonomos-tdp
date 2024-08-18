use std::sync::mpsc::Sender;
use std::{collections::HashMap, io::Error};

use serde_json::Value;
use servidor::commons::subject::subject_matches_token;

use crate::enums::errores_server::ErroresServer;
use crate::structs::jet_stream::consumer::PushConsumer;

use super::file_processing::{read_csv, update_csv_file};

const RESOURCES_PATH: &str = "src/servidor/structs/jet_stream/resources/";
const JETSTREAM_DATA_PATH: &str = "jetstream_data.csv";
const JETSTREAM_CONSUMERS_PATH: &str = "jetstream_consumers.csv";
const CONSUMERS_PATH: &str = "consumers.csv";

type ParsedConsumerData = (String, String, Vec<(String, usize)>);

#[derive(Debug)]
/// Responsable de almacenar mensajes y enviarlos a los consumidores para garantizar QoS
pub struct JetStream {
    pub subjects: Vec<String>,
    messages: Vec<(String, usize)>,
    pub name: String,
    consumers: Vec<PushConsumer>,
    id_counter: usize,
    resources_path: String,
    data_path: String,
    consumers_path: String,
    jet_stream_consumers_path: String,
}

impl JetStream {
    fn _from_json(
        json: &str,
        resources_path: &str,
        data_path: &str,
        consumers_path: &str,
        jet_stream_consumers_path: &str,
    ) -> Result<JetStream, ErroresServer> {
        println!("Json antes: {:?}", json);
        let json: Value = serde_json::from_str(json).map_err(|_| ErroresServer::InvalidJson)?;
        let mut subjects = Vec::new();
        println!("Json: {:?}", json);
        let json_subjects = json["subjects"].as_array();
        let name = json["name"].as_str();
        if let (Some(json_subjects), Some(name)) = (json_subjects, name) {
            println!("Json subjects: {:?}, name: {}", json_subjects, name);
            for subject in json_subjects {
                if let Some(subject) = subject.as_str() {
                    subjects.push(subject.to_string());
                    continue;
                }
                return Err(ErroresServer::InvalidJson);
            }

            let js = JetStream {
                subjects,
                messages: Vec::new(),
                name: name.to_string(),
                consumers: Vec::new(),
                id_counter: 0,
                resources_path: resources_path.to_string(),
                data_path: data_path.to_string(),
                consumers_path: consumers_path.to_string(),
                jet_stream_consumers_path: jet_stream_consumers_path.to_string(),
            };

            return Ok(js);
        }
        Err(ErroresServer::InvalidJson)
    }

    /// Crea un nuevo JetStream a partir de un JSON
    pub fn from_json(json: &str) -> Result<JetStream, ErroresServer> {
        JetStream::_from_json(
            json,
            RESOURCES_PATH,
            JETSTREAM_DATA_PATH,
            CONSUMERS_PATH,
            JETSTREAM_CONSUMERS_PATH,
        )
    }

    /// Envia todos los mensajes almacenados a los consumidores
    fn send_data_to_consumers(&mut self) {
        println!(
            "Enviando todos los mensajes que tiene el js. Mensajes {:?}",
            self.messages
        );
        for (msg, id) in self.messages.iter() {
            println!("Enviando mensaje: {:?}", msg);
            self.consumers.iter().for_each(|x| {
                if x.send(msg, *id).is_err() {
                    println!("Error al enviar mensaje a consumer: {:?}", x.name);
                }
            });
        }
    }

    /// Almacena un mensaje y lo envia a los consumidores
    pub fn store_msg(&mut self, msg: &str) {
        println!("Mensaje almacenado en stream: {:?}", self.messages);
        self.messages.push((msg.to_string(), self.id_counter));
        self.consumers.iter().for_each(|x| {
            let _ = x.send(msg, self.id_counter);
        });
        self.id_counter += 1;
        let _ = self.store_jetstream_data();
        let _ = self.store_jetstream_consumer_rel();
    }

    /// Verifica si `subject` matchea con alguno de los subjects del JetStream
    pub fn matches(&self, subject: &str) -> bool {
        println!(
            "JETSTREAM: subject: {:?}, self.subject: {:?}",
            subject, self.subjects
        );
        self.subjects
            .iter()
            .any(|x| subject_matches_token(x, subject))
    }

    /// Agrega un consumidor al JetStream
    pub fn add_consumer(&mut self, consumer: PushConsumer) {
        let exists = self.consumers.iter().any(|x| x.name == consumer.name);
        if exists {
            println!("Consumer ya existe");
            return;
        }

        self.consumers.push(consumer);
        let _ = self.store_jetstream_consumer_rel();
    }

    /// Comunica al consumidor con `consumer_name` que el mensaje con `ack_id` fue recibido
    pub fn ack(&mut self, consumer_name: &str, ack_id: usize) -> Result<(), ErroresServer> {
        let consumer = self
            .consumers
            .iter_mut()
            .find(|x| x.name == consumer_name)
            .ok_or_else(|| ErroresServer::InternalError("Consumer not found".to_string()))?;
        consumer.ack(ack_id)?;
        let path = format!("{}{}", self.resources_path, self.consumers_path);
        let _ = consumer.to_csv(&path);
        Ok(())
    }

    /// Almacena en file storage los datos del JetStream
    pub fn store_jetstream(&self) -> Result<(), Error> {
        self.store_jetstream_data()?;
        self.store_jetstream_consumer_rel()?;
        Ok(())
    }

    /// Almacena los mensajes del JetStream en file storage
    fn store_jetstream_data(&self) -> Result<(), Error> {
        let updated_jetstream_data = self.messages.iter().fold(
            format!("{},{}", self.name, self.subjects[0]),
            |mut acc, x| {
                acc.push_str(&format!(",{} {}", x.1, x.0));
                acc
            },
        );
        println!("Updated jetstream data: {:?}", updated_jetstream_data);
        let path = format!("{}{}", self.resources_path, self.data_path);
        update_csv_file(&path, updated_jetstream_data, self.name.to_owned())?;
        Ok(())
    }

    /// Almacena la relación entre el JetStream y sus consumidores en file storage
    fn store_jetstream_consumer_rel(&self) -> Result<(), Error> {
        let updated_consumer_info =
            self.consumers
                .iter()
                .fold(self.name.to_owned(), |mut acc, x| {
                    acc.push_str(&format!(",{}", x.name));
                    acc
                });
        let path = self.resources_path.to_owned() + &self.jet_stream_consumers_path;
        update_csv_file(&path, updated_consumer_info, self.name.to_owned())?;
        Ok(())
    }

    /// Parsea los datos de los consumidores tomando una linea del archivo CSV
    fn parse_consumers_data(line: &str) -> Result<(String, String, usize), ErroresServer> {
        let mut parts = line.split(',');

        if let (Some(name), Some(delivery_subject), Some(last_ack)) =
            (parts.next(), parts.next(), parts.next())
        {
            if let Ok(last_ack) = last_ack.trim().parse() {
                return Ok((
                    name.trim().to_string(),
                    delivery_subject.trim().to_string(),
                    last_ack,
                ));
            }
        }
        Err(ErroresServer::ParserError)
    }

    /// Carga los consumidores de un archivo CSV
    fn load_consumers(
        notify: Sender<(String, usize)>,
        resources_path: &str,
        consumers_path: &str,
    ) -> Result<Vec<PushConsumer>, ErroresServer> {
        let mut consumers = Vec::new();
        let path = resources_path.to_string() + consumers_path;
        if let Ok(consumers_data) = read_csv(&path) {
            for line in consumers_data {
                if let Ok((name, delivery_subject, last_acknoledged)) =
                    Self::parse_consumers_data(&line)
                {
                    if let Ok(consumer) = PushConsumer::init(
                        &delivery_subject,
                        &name,
                        notify.clone(),
                        last_acknoledged,
                        &(resources_path.to_owned() + consumers_path),
                    ) {
                        consumers.push(consumer);
                    }
                }
            }
        }
        Ok(consumers)
    }

    /// Separa los consumidores en dos grupos, los que matchean con `consumer_names` y los que no
    fn separate_consumers(
        all_consumers: Vec<PushConsumer>,
        consumer_names: Vec<String>,
    ) -> (Vec<PushConsumer>, Vec<PushConsumer>) {
        let (matching_consumers, remaining_consumers) = all_consumers
            .into_iter()
            .partition(|consumer| consumer_names.contains(&consumer.name));
        (matching_consumers, remaining_consumers)
    }

    /// Parsea los datos de un JetStream tomando una linea del archivo CSV
    fn parse_js_data(line: &str) -> Result<ParsedConsumerData, ErroresServer> {
        let mut parts = line.split(',');
        if let (Some(name), Some(subject)) = (parts.next(), parts.next()) {
            let mut messages = Vec::new();
            for msg in parts {
                let mut parts = msg.split_whitespace();
                if let Some(id) = parts.next() {
                    if let Ok(id) = id.parse() {
                        let msg = parts.collect::<Vec<&str>>().join(" ");
                        messages.push((msg, id));
                    }
                }
            }
            return Ok((
                name.trim().to_string(),
                subject.trim().to_string(),
                messages,
            ));
        }
        Err(ErroresServer::ParserError)
    }

    fn _load_file_storage(
        notify: Sender<(String, usize)>,
        resources_path: &str,
        data_path: &str,
        consumers_path: &str,
        jet_stream_consumers_path: &str,
    ) -> Result<Vec<JetStream>, ErroresServer> {
        let mut jetstreams = Vec::new();

        let mut consumers =
            JetStream::load_consumers(notify, resources_path, consumers_path).unwrap_or_default();
        let mut jetstream_consumers = HashMap::new();

        let js_consumers_path = resources_path.to_string() + jet_stream_consumers_path;
        let js_data_path = resources_path.to_string() + data_path;

        if let Ok(jetstream_consumers_data) = read_csv(&js_consumers_path) {
            for line in jetstream_consumers_data {
                let mut parts = line.split(',');
                if let Some(jetstream_name) = parts.next() {
                    let consumer_names: Vec<String> = parts.map(|x| x.trim().to_string()).collect();
                    let (matching_consumers, remaining) =
                        Self::separate_consumers(consumers, consumer_names);
                    jetstream_consumers
                        .insert(jetstream_name.trim().to_string(), matching_consumers);

                    consumers = remaining;
                }
            }
        }

        if let Ok(js_data) = read_csv(&js_data_path) {
            for line in js_data {
                if let Ok((name, subject, messages)) = Self::parse_js_data(&line) {
                    let id_counter = messages.iter().map(|x| x.1 + 1).max().unwrap_or(0);
                    println!("Messages: {:?}", messages);
                    println!("Id counter: {:?}", id_counter);
                    if let Some(consumers) = jetstream_consumers.remove(&name) {
                        let mut jetstream = JetStream {
                            subjects: vec![subject.to_string()],
                            messages,
                            id_counter,
                            name: name.to_string(),
                            consumers,
                            resources_path: resources_path.to_string(),
                            data_path: data_path.to_string(),
                            consumers_path: consumers_path.to_string(),
                            jet_stream_consumers_path: jet_stream_consumers_path.to_string(),
                        };
                        jetstream.send_data_to_consumers();
                        jetstreams.push(jetstream);
                    }
                }
            }
        }
        Ok(jetstreams)
    }

    /// Carga los JetStreams desde file storage
    pub fn load_file_storage(
        notify: Sender<(String, usize)>,
    ) -> Result<Vec<JetStream>, ErroresServer> {
        JetStream::_load_file_storage(
            notify,
            RESOURCES_PATH,
            JETSTREAM_DATA_PATH,
            CONSUMERS_PATH,
            JETSTREAM_CONSUMERS_PATH,
        )
    }
}

#[cfg(test)]
mod integration_tests_jet_stream {

    use icwt_crypto::icwt_crypto::crypto::encrypt;

    use super::*;
    use std::{
        sync::mpsc::{channel, RecvTimeoutError},
        thread,
        time::Duration,
    };

    const TEST_PATH_JS_DATA: &str = "jetstream_data_{test_number}_test.csv";
    const TEST_PATH_JS_CONSUMERS: &str = "jetstream_consumers_{test_number}_test.csv";
    const TEST_PATH_CONSUMERS: &str = "consumers_{test_number}_test.csv";

    fn get_path_for_test(test_number: usize) -> (String, String, String) {
        (
            TEST_PATH_JS_DATA.replace("{test_number}", &test_number.to_string()),
            TEST_PATH_JS_CONSUMERS.replace("{test_number}", &test_number.to_string()),
            TEST_PATH_CONSUMERS.replace("{test_number}", &test_number.to_string()),
        )
    }

    fn remove_files(test_number: usize) {
        let test_paths = get_path_for_test(test_number);
        let _ = std::fs::remove_file(RESOURCES_PATH.to_string() + &test_paths.0);
        let _ = std::fs::remove_file(RESOURCES_PATH.to_string() + &test_paths.1);
        let _ = std::fs::remove_file(RESOURCES_PATH.to_string() + &test_paths.2);
    }

    fn write_file(test_number: usize, content: &str, path: &str) {
        let path =
            RESOURCES_PATH.to_string() + &path.replace("{test_number}", &test_number.to_string());
        std::fs::write(path, content).unwrap();
    }

    struct BeforeEachSetup {
        test_num: usize,
        js_data_path: String,
        js_consumers_path: String,
        consumers_path: String,
        resources_path: String,
    }

    fn _read_csv(path: &str) -> Vec<String> {
        read_csv(&(RESOURCES_PATH.to_string() + path)).unwrap()
    }

    fn before_each(test_number: usize) -> BeforeEachSetup {
        remove_files(test_number);
        let test_paths = get_path_for_test(test_number);

        BeforeEachSetup {
            test_num: test_number,
            js_data_path: test_paths.0,
            js_consumers_path: test_paths.1,
            consumers_path: test_paths.2,
            resources_path: RESOURCES_PATH.to_string(),
        }
    }

    fn after_each(test_number: usize) {
        remove_files(test_number);
    }

    #[test]
    fn test_jetstream_from_json() {
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";

        let js = JetStream::from_json(json).unwrap();

        assert_eq!(js.name, "jetstream1");
        assert_eq!(
            js.subjects,
            vec!["subject1".to_string(), "subject2".to_string()]
        );
    }

    #[test]
    fn test_jetstream_from_json_invalid_json() {
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"";

        let js = JetStream::from_json(json);

        assert!(matches!(js, Err(ErroresServer::InvalidJson)));
    }

    #[test]
    fn test_jetstream_store_msg() {
        let setup = before_each(3);
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";
        let mut js = JetStream::_from_json(
            json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();

        js.store_msg("hola");

        let js_consumers_file = _read_csv(&setup.js_consumers_path);
        let js_data_file = _read_csv(&setup.js_data_path);
        assert_eq!("jetstream1", js_consumers_file[0]);
        assert_eq!("jetstream1,subject1,0 hola", js_data_file[0]);
        assert_eq!(js.messages, vec![("hola".to_string(), 0)]);
        after_each(setup.test_num);
    }

    #[test]
    fn test_jetstream_store_multiple_msgs() {
        let setup = before_each(4);
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";
        let mut js = JetStream::_from_json(
            json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();

        js.store_msg("hola");
        js.store_msg("chau");

        let js_consumers_file = _read_csv(&setup.js_consumers_path);
        let js_data_file = _read_csv(&setup.js_data_path);
        assert_eq!("jetstream1", js_consumers_file[0]);
        assert_eq!("jetstream1,subject1,0 hola,1 chau", js_data_file[0]);
        assert_eq!(
            js.messages,
            vec![("hola".to_string(), 0), ("chau".to_string(), 1)]
        );
        after_each(setup.test_num);
    }

    #[test]
    fn test_multiple_jetstreams() {
        let setup = before_each(5);
        let first_json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";
        let second_json = "{\"subjects\":[\"subject3\",\"subject4\"],\"name\":\"jetstream2\"}";
        let mut first_js = JetStream::_from_json(
            &first_json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();
        let mut second_js = JetStream::_from_json(
            &second_json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();

        first_js.store_msg("mensaje en jetstream1");
        second_js.store_msg("mensaje en jetstream2");
        first_js.store_msg("otro mensaje en jetstream1");

        assert_eq!(
            first_js.messages,
            vec![
                ("mensaje en jetstream1".to_string(), 0),
                ("otro mensaje en jetstream1".to_string(), 1)
            ]
        );
        assert_eq!(
            second_js.messages,
            vec![("mensaje en jetstream2".to_string(), 0)]
        );
        assert_eq!(
            "jetstream1,subject1,0 mensaje en jetstream1,1 otro mensaje en jetstream1",
            _read_csv(&setup.js_data_path)[0]
        );
        assert_eq!(
            "jetstream2,subject3,0 mensaje en jetstream2",
            _read_csv(&setup.js_data_path)[1]
        );
        after_each(setup.test_num);
    }

    #[test]
    fn test_jetstream_add_consumer() {
        let setup = before_each(6);
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";
        let mut js = JetStream::_from_json(
            json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();
        let (tx, _) = channel();
        let consumer = PushConsumer::init(
            "subject1",
            "consumer1",
            tx,
            0,
            &(setup.resources_path.to_owned() + &setup.consumers_path),
        )
        .unwrap();

        js.add_consumer(consumer);

        let js_consumers_file = _read_csv(&setup.js_consumers_path);
        let consumers_file = _read_csv(&setup.consumers_path);
        assert_eq!("jetstream1,consumer1", js_consumers_file[0]);
        assert_eq!("consumer1,subject1,0", consumers_file[0]);
        assert_eq!(js.consumers.len(), 1);
        after_each(setup.test_num);
    }

    #[test]
    fn test_consumer_notifies_when_storing_msg() {
        let setup = before_each(7);
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";
        let mut js = JetStream::_from_json(
            json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();
        let (tx, rx) = channel();
        let consumer = PushConsumer::init(
            "subject1",
            "consumer1",
            tx,
            0,
            &(setup.resources_path.to_owned() + &setup.consumers_path),
        )
        .unwrap();
        let enc_len = encrypt("0.hola".as_bytes()).unwrap().len();
        let expected_msg = format!("PUB subject1 {}\r\n0.hola\r\n", enc_len);
        js.add_consumer(consumer);

        js.store_msg("hola");

        assert_eq!(rx.recv().unwrap(), (expected_msg, 0));
        after_each(setup.test_num);
    }

    #[test]
    fn test_consumer_notifies_multiple_times_if_not_ack() {
        let setup = before_each(8);
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";
        let mut js = JetStream::_from_json(
            json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();
        let (tx, rx) = channel();
        let consumer = PushConsumer::init(
            "subject1",
            "consumer1",
            tx,
            0,
            &(setup.resources_path.to_owned() + &setup.consumers_path),
        )
        .unwrap();
        let expected_msg = format!(
            "PUB subject1 {}\r\n0.hola\r\n",
            encrypt("0.hola".as_bytes()).unwrap().len()
        );
        js.add_consumer(consumer);

        js.store_msg("hola");

        assert_eq!(rx.recv().unwrap(), (expected_msg.to_owned(), 0));
        thread::sleep(Duration::from_secs(10));
        assert_eq!(rx.recv().unwrap(), (expected_msg, 0));
        after_each(setup.test_num);
    }

    #[test]
    fn test_consumer_stops_notifying_if_ack() {
        let setup = before_each(9);
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";
        let mut js = JetStream::_from_json(
            json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();
        let (tx, rx) = channel();
        let consumer = PushConsumer::init(
            "subject1",
            "consumer1",
            tx,
            0,
            &(setup.resources_path.to_owned() + &setup.consumers_path),
        )
        .unwrap();
        js.add_consumer(consumer);

        js.store_msg("hola");
        rx.recv().unwrap();
        js.ack("consumer1", 1).unwrap();

        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(10)),
            Err(RecvTimeoutError::Timeout)
        ));
        after_each(setup.test_num);
    }

    #[test]
    fn test_multiple_consumers_stored_correctly() {
        let setup = before_each(10);
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";
        let mut js = JetStream::_from_json(
            json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();
        let (tx1, _) = channel();
        let (tx2, _) = channel();
        let consumer1 = PushConsumer::init(
            "subject1",
            "consumer1",
            tx1,
            0,
            &(setup.resources_path.to_owned() + &setup.consumers_path),
        )
        .unwrap();
        let consumer2 = PushConsumer::init(
            "subject1",
            "consumer2",
            tx2,
            0,
            &(setup.resources_path.to_owned() + &setup.consumers_path),
        )
        .unwrap();

        js.add_consumer(consumer1);
        js.add_consumer(consumer2);

        assert_eq!(
            "jetstream1,consumer1,consumer2",
            _read_csv(&setup.js_consumers_path)[0]
        );
        assert_eq!("consumer1,subject1,0", _read_csv(&setup.consumers_path)[0]);
        assert_eq!("consumer2,subject1,0", _read_csv(&setup.consumers_path)[1]);
        after_each(setup.test_num);
    }

    #[test]
    fn test_multiple_consumers_send_messages() {
        let setup = before_each(11);
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";
        let mut js = JetStream::_from_json(
            json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();
        let (tx1, rx1) = channel();
        let (tx2, rx2) = channel();
        let consumer1 = PushConsumer::init(
            "subject1",
            "consumer1",
            tx1,
            0,
            &(setup.resources_path.to_owned() + &setup.consumers_path),
        )
        .unwrap();
        let consumer2 = PushConsumer::init(
            "subject1",
            "consumer2",
            tx2,
            0,
            &(setup.resources_path.to_owned() + &setup.consumers_path),
        )
        .unwrap();
        let expected_msg = format!(
            "PUB subject1 {}\r\n0.hola consumers\r\n",
            encrypt("0.hola consumers".as_bytes()).unwrap().len()
        );
        js.add_consumer(consumer1);
        js.add_consumer(consumer2);

        js.store_msg("hola consumers");

        assert_eq!(rx1.recv().unwrap(), (expected_msg.to_owned(), 0));
        assert_eq!(rx2.recv().unwrap(), (expected_msg, 0));
        after_each(setup.test_num);
    }

    #[test]
    fn test_multiple_consumers_one_ack_the_other_still_sends_old_msg() {
        let setup = before_each(12);
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";
        let mut js = JetStream::_from_json(
            json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();
        let (tx1, rx1) = channel();
        let (tx2, rx2) = channel();
        let consumer1 = PushConsumer::init(
            "subject1",
            "consumer1",
            tx1,
            0,
            &(setup.resources_path.to_owned() + &setup.consumers_path),
        )
        .unwrap();
        let consumer2 = PushConsumer::init(
            "subject1",
            "consumer2",
            tx2,
            0,
            &(setup.resources_path.to_owned() + &setup.consumers_path),
        )
        .unwrap();
        let expected_msg_1 = format!(
            "PUB subject1 {}\r\n0.hola consumers\r\n",
            encrypt("0.hola consumers".as_bytes()).unwrap().len()
        );
        let expected_msg_2 = format!(
            "PUB subject1 {}\r\n1.chauuu consumers\r\n",
            encrypt("1.chauuu consumers".as_bytes()).unwrap().len()
        );
        js.add_consumer(consumer1);
        js.add_consumer(consumer2);

        js.store_msg("hola consumers");
        js.store_msg("chauuu consumers");
        rx1.recv().unwrap();
        rx2.recv().unwrap();
        js.ack("consumer1", 0).unwrap();

        thread::sleep(Duration::from_secs(10));
        assert_eq!(rx2.recv().unwrap(), (expected_msg_1, 0));
        assert_eq!(rx1.recv().unwrap(), (expected_msg_2, 0));
        after_each(setup.test_num);
    }

    #[test]
    fn test_load_from_file_storage() {
        let setup = before_each(13);
        write_file(
            setup.test_num,
            "consumer1,subject1,2\nconsumer2,subject1,1\n",
            &setup.consumers_path,
        );
        write_file(
            setup.test_num,
            "jetstream1,consumer1,consumer2\njetstream2",
            &setup.js_consumers_path,
        );
        write_file(
            setup.test_num,
            "jetstream1,subject1,0 hola,1 como va?,2 todo bien,3 chau\njetstream2,subject2,0 hola2",
            &setup.js_data_path,
        );
        let (tx1, _) = channel();

        let jet_streams = JetStream::_load_file_storage(
            tx1,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();

        assert_eq!(jet_streams.len(), 2);
        assert_eq!(jet_streams[0].name, "jetstream1");
        assert_eq!(jet_streams[0].subjects, vec!["subject1".to_string()]);
        assert_eq!(
            jet_streams[0].messages,
            vec![
                ("hola".to_string(), 0),
                ("como va?".to_string(), 1),
                ("todo bien".to_string(), 2),
                ("chau".to_string(), 3)
            ]
        );
        assert_eq!(jet_streams[1].name, "jetstream2");
        assert_eq!(jet_streams[1].subjects, vec!["subject2".to_string()]);
        assert_eq!(jet_streams[1].messages, vec![("hola2".to_string(), 0)]);
        after_each(setup.test_num);
    }

    #[test]
    fn test_loaded_consumers_preserve_state() {
        let setup = before_each(14);
        write_file(
            setup.test_num,
            "consumer1,delivery_subject_1,2\n",
            &setup.consumers_path,
        );
        write_file(
            setup.test_num,
            "jetstream1,consumer1\njetstream2",
            &setup.js_consumers_path,
        );
        write_file(
            setup.test_num,
            "jetstream1,subject1,0 hola,1 como va?,2 todo bien,3 chau\njetstream2,subject2,0 hola2",
            &setup.js_data_path,
        );
        let expected_msg = format!(
            "PUB delivery_subject_1 {}\r\n2.todo bien\r\n",
            encrypt("2.todo bien".as_bytes()).unwrap().len()
        );
        let (tx1, rx1) = channel();

        JetStream::_load_file_storage(
            tx1,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();

        assert_eq!(rx1.recv().unwrap(), (expected_msg, 0));
        after_each(setup.test_num);
    }

    #[test]
    fn test_acknoledging_multiple_times_same_msg_still_send_next_msgs() {
        let setup = before_each(15);
        let json = "{\"subjects\":[\"subject1\",\"subject2\"],\"name\":\"jetstream1\"}";
        let mut js = JetStream::_from_json(
            json,
            &setup.resources_path,
            &setup.js_data_path,
            &setup.consumers_path,
            &setup.js_consumers_path,
        )
        .unwrap();
        let (tx, rx) = channel();
        let consumer = PushConsumer::init(
            "delivery_subject",
            "consumer1",
            tx,
            0,
            &(setup.resources_path.to_owned() + &setup.consumers_path),
        )
        .unwrap();
        let expected_msg = format!(
            "PUB delivery_subject {}\r\n1.como va?\r\n",
            encrypt("1.como va?".as_bytes()).unwrap().len()
        );
        js.add_consumer(consumer);
        js.store_msg("buenas");
        js.store_msg("como va?");

        js.ack("consumer1", 0).unwrap();
        js.ack("consumer1", 0).unwrap();
        js.ack("consumer1", 0).unwrap();
        rx.recv().unwrap();

        assert_eq!(rx.recv().unwrap(), (expected_msg, 0));
        after_each(setup.test_num);
    }
}
