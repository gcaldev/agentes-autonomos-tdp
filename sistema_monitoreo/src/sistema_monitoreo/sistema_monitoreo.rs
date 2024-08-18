use super::camara::Camara;
use super::dron::Dron;
use super::errores::SistemaMonitoreoError;
use super::posicion::Posicion;
use crate::sistema_monitoreo::estado::Estado;
use crate::sistema_monitoreo::incidente::Incidente;
use cliente::cliente::client::NatsClient;
use cliente::cliente::iclient::INatsClient;
use cliente::cliente::user::User;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use std::io::Error;
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

#[derive(Deserialize, Debug)]
/// Responsable de proveer la configuracion del sistema de monitoreo
struct Config {
    address_client: String,
    pub_incidentes: String,
    sub_incidentes_ia: String,
    sub_drones: String,
    sub_camara: String,
}

impl Config {
    fn from_file(file_path: &str) -> Result<Self, Error> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let config = serde_json::from_reader(reader)?;
        Ok(config)
    }
}

/// Parsea el proximo numero de `iter`
fn parsear(iter: &mut std::str::SplitWhitespace) -> Option<f64> {
    if let Some(numero1) = iter.next() {
        if let Ok(numero2) = numero1.parse::<f64>() {
            return Some(numero2);
        }
    }
    None
}

/// Establece el estado de la camara a ahorro
fn ahorro(camaras: &mut HashMap<String, Camara>, id: &str, lat: f64, long: f64) {
    if let Some(cam) = camaras.get_mut(id) {
        cam.cambiar_estado("ahorro".to_string());
        println!("Cambio a modo ahorro mi camara");
    } else {
        let posicion = Posicion::new(lat, long);
        camaras.insert(
            id.to_string(),
            Camara::new(id.to_string(), posicion, "ahorro".to_string()),
        );
        println!("Agrego la nueva camara");
    }
}

/// Establece el estado de la camara a grabando
fn grabando(camaras: &mut HashMap<String, Camara>, id: &str) {
    if let Some(cam) = camaras.get_mut(id) {
        cam.cambiar_estado("grabando".to_string());
        println!("paso la camara a grabando");
    }
}

/// Elimina la camara
fn eliminado(camaras: &mut HashMap<String, Camara>, id: &str) {
    camaras.remove(id);
}

/// Actualiza el estado, posicion y bateria del dron
fn actualizar(
    drones: &mut HashMap<String, Dron>,
    id: &str,
    estado: &str,
    lat: f64,
    long: f64,
    bateria: f64,
) {
    if let Some(dron) = drones.get_mut(id) {
        dron.set_estado(estado.to_string());
        dron.set_posicion(lat, long);
        dron.set_bateria(bateria);
        println!("dron con id: {}, actualizado", id);
    } else {
        crear(drones, id, estado, lat, long, bateria);
    }
}

/// Crea un nuevo dron
fn crear(
    drones: &mut HashMap<String, Dron>,
    id: &str,
    estado: &str,
    lat: f64,
    long: f64,
    bateria: f64,
) {
    let posicion = Posicion::new(lat, long);
    drones.insert(
        id.to_string(),
        Dron::new(id.to_string(), posicion, estado.to_string(), bateria),
    );
    println!("Agrego nuevo dron");
}

/// Maneja la resolucion de un incidente retornando el payload a publicar
fn resolviendo(
    hash: &mut HashMap<String, Vec<String>>,
    id_dron: &str,
    latitud: f64,
    longitud: f64,
    mut iter: std::str::SplitWhitespace,
    lista_de_incidentes_clone: &Arc<Mutex<HashMap<String, Incidente>>>,
) -> Option<String> {
    if let Some(some_id1) = iter.next() {
        if let Ok(id_incidente) = some_id1.parse::<i32>() {
            if let Ok(mut lista) = lista_de_incidentes_clone.lock() {
                if let Some(incidente) = lista.get_mut(&id_incidente.to_string()) {
                    if let Some(vec) = hash.get_mut(&id_incidente.to_string()) {
                        if matches!(incidente.get_estado(), Estado::Cancelado) {
                            return None;
                        } else if vec.len() < 2 {
                            if !vec.contains(&id_dron.to_string()) {
                                vec.push(id_dron.to_string());
                            }
                        } else {
                            incidente.set_estado(Estado::Cancelado);
                            return Some(format!(
                                "{} {} {} {} {} {}",
                                id_incidente,
                                latitud,
                                longitud,
                                "activo",
                                vec[0],
                                vec[1],
                            ));                    
                        }
                        return None;
                    } else {
                        println!("id de incidente incorrecto, no existe");
                    }
                }
            }
        }
    }
    None
}

/// Elimina un dron
fn eliminar_drone(drones: &mut HashMap<String, Dron>, id: &str) {
    drones.remove(id);
    println!("elimino un dron");
}

/// Responsable de monitorear los incidentes, camaras y drones
pub struct SistemaDeMonitoreo {
    pub lista_de_incidentes: Arc<Mutex<HashMap<String, Incidente>>>,
    pub camaras: Arc<Mutex<HashMap<String, Camara>>>,
    pub drones: Arc<Mutex<HashMap<String, Dron>>>,
    pub client: Arc<Mutex<Box<dyn INatsClient + Send>>>,
    pub id_count: Arc<Mutex<usize>>,
    config: Config,
    incidentes_drones: Arc<Mutex<HashMap<String, Vec<String>>>>,
}

impl SistemaDeMonitoreo {
    /// Inicializa el sistema de monitoreo
    fn init(
        nats: Box<dyn INatsClient + Send>,
        config: Config,
    ) -> Result<Self, SistemaMonitoreoError> {
        let camaras = Arc::new(Mutex::new(HashMap::<String, Camara>::new()));
        let camaras_clone = camaras.clone();

        let drones = Arc::new(Mutex::new(HashMap::<String, Dron>::new()));
        let dron_clone = drones.clone();

        let cliente = Arc::new(Mutex::new(nats));
        let nats_now = cliente.clone();
        let cliente_drones = cliente.clone();
        let nats_inc_ia = cliente.clone();

        let pub_subject = config.pub_incidentes.clone();

        let lista_de_incidentes: Arc<Mutex<HashMap<String, Incidente>>> =
            Arc::new(Mutex::new(HashMap::<String, Incidente>::new()));
        let lista_de_incidentes_clone = lista_de_incidentes.clone();
        let lista_de_incidentes_ia_clone = lista_de_incidentes.clone();


        //id incidente y id's de 2 drones
        let incidentes_drones: Arc<Mutex<HashMap<String, Vec<String>>>> =
            Arc::new(Mutex::new(HashMap::<String, Vec<String>>::new()));
        let indentes_drones_sub_drones = incidentes_drones.clone();
        let indentes_drones_inc_ia = incidentes_drones.clone();


        let mut nats_client = if let Ok(cl) = nats_now.lock() {
            cl
        } else {
            return Err(SistemaMonitoreoError::InternalError);
        };

        let jetstream_name = format!("JetStream{}", config.sub_camara);
        let camara_consumer = format!("Sistema_Monitoreo_Consumer{}", config.sub_camara);
        let delivery_subject = format!("Sistema_Monitoreo_Delivery{}", config.sub_camara);
        nats_client
            .create_stream(&config.sub_camara, &jetstream_name)
            .map_err(|_| SistemaMonitoreoError::InternalError)?;
        nats_client
            .create_and_consume(
                &jetstream_name,
                &camara_consumer,
                &delivery_subject,
                Box::new(move |payload| {
                    let mut iter: std::str::SplitWhitespace = payload.0.split_whitespace();

                    let latitud;
                    if let Some(lat) = parsear(&mut iter) {
                        latitud = lat;
                    } else {
                        println!("Error al parsear longitud y latitud");
                        return;
                    }

                    let longitud;
                    if let Some(long) = parsear(&mut iter) {
                        longitud = long;
                    } else {
                        println!("Error al parsear longitud y latitud");
                        return;
                    }

                    let estado = if let Some(est) = iter.next() {
                        est
                    } else {
                        return;
                    };
                    let id = if let Some(id) = iter.next() {
                        id
                    } else {
                        return;
                    };

                    if let Ok(mut camaras) = camaras_clone.lock() {
                        match estado {
                            "ahorro" => ahorro(&mut camaras, id, latitud, longitud),
                            "grabando" => grabando(&mut camaras, id),
                            "eliminado" => eliminado(&mut camaras, id),
                            _ => println!("Estado inexistente"),
                        }
                    }
                }),
            )
            .map_err(|_| SistemaMonitoreoError::InternalError)?;

        let jetstream_name = "JetStream-Sub_Drones";
        let dron_consumer = "DronConsumer";
        let delivery_subject = "delivery_dron";

        nats_client
            .create_stream(&config.sub_drones, jetstream_name)
            .map_err(|_| SistemaMonitoreoError::InternalError)?;
        nats_client
            .create_and_consume(
                jetstream_name,
                dron_consumer,
                delivery_subject,
                Box::new(move |payload| {
                    let mut iter = payload.0.split_whitespace();

                    let latitud;
                    if let Some(lat) = parsear(&mut iter) {
                        latitud = lat;
                    } else {
                        println!("Error al parsear longitud y latitud");
                        return;
                    }

                    let longitud;
                    if let Some(long) = parsear(&mut iter) {
                        longitud = long;
                    } else {
                        println!("Error al parsear longitud y latitud");
                        return;
                    }

                    let estado = if let Some(est) = iter.next() {
                        est
                    } else {
                        return;
                    };
                    let id = if let Some(id) = iter.next() {
                        id
                    } else {
                        return;
                    };

                    let bateria;
                    if let Some(bat) = parsear(&mut iter) {
                        bateria = bat;
                    } else {
                        println!("Error al parsear la bateria");
                        return;
                    }

                    let estados = [
                        "recargando",
                        "encamino",
                        "enCaminoAreaOp",
                        "enCaminoBase",
                        "enCaminoIncidente",
                        "patrullando",
                        "eliminado",
                        "resolviendo",
                    ];

                    if !estados.contains(&estado) {
                        println!("Error estado invalido, no existe");
                        return;
                    }

                    if let Ok(mut drones) = dron_clone.lock() {
                        actualizar(&mut drones, id, estado, latitud, longitud, bateria); //crea o actualiza al dron

                        match estado {
                            "resolviendo" => {
                                if let Ok(mut hash) = indentes_drones_sub_drones.lock() {
                                    if let Ok(mut cliente) = cliente_drones.lock() {
                                        if let Some(payload) = resolviendo(
                                            &mut hash,
                                            id,
                                            latitud,
                                            longitud,
                                            iter,
                                            &lista_de_incidentes_clone,
                                        ) {
                                            if let Err(e) =
                                                cliente.publish(&pub_subject, Some(&payload), None)
                                            {
                                                println!("Error al publicar: {}", e);
                                            }

                                            drop(cliente);

                                            // id_incidente - lat - long - estado - id_dron1 - id_dron2
                                            let payload_parseado: Vec<&str> =
                                                payload.split(' ').collect();
                                            let tiempo;

                                            if let Ok(mut lista) = lista_de_incidentes_clone.lock()
                                            {
                                                if let Some(incidente) =
                                                    lista.get_mut(payload_parseado[0])
                                                {
                                                    incidente.set_estado(Estado::Activo);
                                                    tiempo = incidente.get_tiempo_resolucion();
                                                } else {
                                                    println!("Id de incidente invalido, no existe");
                                                    return;
                                                }
                                            } else {
                                                println!("Error lock envenedado");
                                                return;
                                            }
                                            let lista = lista_de_incidentes_clone.clone();
                                            let payload_th = payload.clone();
                                            let cliente_th = cliente_drones.clone();
                                            let subject_th = pub_subject.clone();

                                            //crear thread
                                            let builder = thread::Builder::new().name(format!(
                                                "thread-incidente-id:{}",
                                                payload_parseado[0]
                                            ));

                                            let _ = builder.spawn(move || {
                                                thread::sleep(Duration::from_secs(tiempo));

                                                let payload_parse: Vec<&str> =
                                                    payload_th.split(' ').collect();

                                                if let Ok(mut cliente) = cliente_th.lock() {
                                                    let payload = &format!(
                                                        "{} {} {} {}",
                                                        payload_parse[0],
                                                        payload_parse[1],
                                                        payload_parse[2],
                                                        "cancelado",
                                                    );

                                                    if let Ok(mut lista) = lista.lock() {
                                                        lista.remove(&payload_parse[0].to_string());
                                                    }

                                                    if let Err(e) = cliente.publish(
                                                        &subject_th,
                                                        Some(payload),
                                                        None,
                                                    ) {
                                                        println!(
                                                            "Ocurrio un errror al publicar: {}",
                                                            e
                                                        );
                                                    }
                                                } else {
                                                    println!("Error lock envenenado");
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                            "eliminado" => eliminar_drone(&mut drones, id),
                            _ => (),
                        }
                    } else {
                        println!("Lock envenenado");
                    }
                }),
            )
            .map_err(|_| SistemaMonitoreoError::InternalError)?;

            let id_count = 0;
            let id_count_save = Arc::new(Mutex::new(id_count));
            let id_count_th = id_count_save.clone();
            let pub_inc = config.pub_incidentes.clone();

            let jetstream_name = format!("JetStream{}", config.sub_incidentes_ia);
            let ia_consumer = format!("Sistema_Monitoreo_Consumer{}", config.sub_incidentes_ia);
            let delivery_subject = format!("Sistema_Monitoreo_Delivery{}", config.sub_incidentes_ia);
            nats_client
                .create_stream(&config.sub_incidentes_ia, &jetstream_name)
                .map_err(|_| SistemaMonitoreoError::InternalError)?;
            nats_client
                .create_and_consume(
                    &jetstream_name,
                    &ia_consumer,
                    &delivery_subject,
                    Box::new(move |payload| {

                        let mut iter = payload.0.split_whitespace();

                        let latitud;
                        if let Some(lat) = parsear(&mut iter) {
                            latitud = lat;
                        } else {
                            println!("Error al parsear longitud y latitud");
                            return;
                        }

                        let longitud;
                        if let Some(long) = parsear(&mut iter) {
                            longitud = long;
                        } else {
                            println!("Error al parsear longitud y latitud");
                            return;
                        }

                        if let Ok(mut id_count) = id_count_th.lock(){
                            let id = id_count.to_string();
                            let tiempo = 10;
                            let incidente = Incidente::new(tiempo, latitud, longitud, &id);
                            println!("publico incidente");

                            let payload = format!(
                                "{} {} {} {}",
                                id,
                                latitud,
                                longitud,
                                incidente.get_estado().as_str()
                            );

                            if let Ok(mut cl) = nats_inc_ia.lock() {
                                if cl.publish(&pub_inc, Some(&payload), None).is_ok() {
                                    
                                    if let Ok(mut lista) = lista_de_incidentes_ia_clone.lock(){
                                        lista.insert(id.to_owned(), incidente);
                                    }
                            
                                    if let Ok(mut hash) = indentes_drones_inc_ia.lock() {
                                        hash.insert(id, Vec::new());
                                    }
                            
                                    *id_count += 1;
                                }
                            } 






                        }
                    }),
                )
                .map_err(|_| SistemaMonitoreoError::InternalError)?;
                drop(nats_client);

        Ok(SistemaDeMonitoreo {
            lista_de_incidentes,
            client: cliente,
            camaras,
            drones,
            id_count: id_count_save,
            config,
            incidentes_drones,
        })
    }

    /// Crea una nueva instancia de SistemaDeMonitoreo
    pub fn new(user: String, password: String) -> Result<Self, SistemaMonitoreoError> {
        let config =
            Config::from_file("conf.json").map_err(|_| SistemaMonitoreoError::AppConfigError)?;

        let writer = TcpStream::connect(&config.address_client)
            .map_err(|_| SistemaMonitoreoError::InternalError)?;
        let reader = writer
            .try_clone()
            .map_err(|_| SistemaMonitoreoError::InternalError)?;

        let nats = NatsClient::new(writer, reader, "logs.txt", Some(User::new(user, password)))
            .map_err(|_| SistemaMonitoreoError::InternalError)?;

        Self::init(Box::new(nats), config)
    }

    /// Crea un nuevo incidente
    pub fn nuevo_incidente(
        &mut self,
        lat: f64,
        long: f64,
        tiempo: u64,
    ) -> Result<String, SistemaMonitoreoError> {
        let id: String = if let Ok(mut id_count) =  self.id_count.lock(){
            let aux = id_count.to_string();
            *id_count += 1;
            aux
        }else{
            return Err(SistemaMonitoreoError::InternalError);
        };

        let incidente = Incidente::new(tiempo, lat, long, &id);
        println!("publico incidente");
        self.publicar_incidente(&incidente)?;

        self
            .lista_de_incidentes
            .lock()
            .map_err(|_| SistemaMonitoreoError::InternalError)?
            .insert(id.to_owned(), incidente);

        self
            .incidentes_drones
            .lock()
            .map_err(|_| SistemaMonitoreoError::InternalError)?
            .insert(id.to_owned(), Vec::new());

        Ok(id)
    }

    /// Publica un incidente en el servidor
    fn publicar_incidente(&mut self, inc: &Incidente) -> Result<(), SistemaMonitoreoError> {
        let Posicion { lat, long } = inc.get_posision();
        let payload = format!(
            "{} {} {} {}",
            inc.get_id(),
            lat,
            long,
            inc.get_estado().as_str()
        );

        if let Ok(mut cl) = self.client.lock() {
            cl.publish(&self.config.pub_incidentes, Some(&payload), None)
                .map_err(|_| SistemaMonitoreoError::InternalError)?;
        }
        Ok(())
    }

    #[allow(clippy::significant_drop_in_scrutinee)]
    /// Elimina un incidente del sistema
    pub fn eliminar_incidente(&mut self, id: &str) -> Result<(), SistemaMonitoreoError> {
        let mut lista = self
            .lista_de_incidentes
            .lock()
            .map_err(|_| SistemaMonitoreoError::InternalError)?;

        let mut inc = match lista.remove(id) {
            Some(incidente) => incidente,
            None => return Err(SistemaMonitoreoError::IdInvalido),
        };

        drop(lista);

        inc.set_estado(Estado::Cancelado);

        if let Ok(mut inc_dr) = self.incidentes_drones.lock() {
            if inc_dr.remove(id).is_none() {
                return Err(SistemaMonitoreoError::IdInvalido);
            }
        }

        self.publicar_incidente(&inc)?;

        Ok(())
    }

    /// Provee el estado actual de los incidentes
    pub fn obtener_info_incidentes(&self) -> Result<Vec<Incidente>, SistemaMonitoreoError> {
        let lista = self
            .lista_de_incidentes
            .lock()
            .map_err(|_| SistemaMonitoreoError::InternalError)?;
        Ok(lista.values().cloned().collect())
    }

    /// Provee la cantidad de incidentes en el sistema
    pub fn obtener_cant_de_incidentes(&self) -> Result<usize, SistemaMonitoreoError> {
        let lista = self
            .lista_de_incidentes
            .lock()
            .map_err(|_| SistemaMonitoreoError::InternalError)?;
        Ok(lista.len())
    }

    /// Provee el estado actual de las camaras
    pub fn obtener_info_camaras(&self) -> Result<Vec<Camara>, SistemaMonitoreoError> {
        self.camaras.lock().map_or_else(|_| Ok(Vec::new()), |camaras| Ok(camaras.values().cloned().collect()))
    }

    /// Provee el estado actual de los drones
    pub fn obtener_info_drones(&self) -> Vec<Dron> {
        self.drones.lock().map_or_else(|_| Vec::new(), |drones| drones.values().cloned().collect())
    }
}

#[cfg(test)]
mod test {
    use cliente::cliente::errors::NatsClientError;
    use cliente::cliente::suscriptor_handler::SuscriptionInfo;

    use super::*;
    use std::thread;
    use std::time::Duration;

    struct MockNatsClient {
        cam_states: Vec<String>,
        publish_err: Option<NatsClientError>,
    }

    impl MockNatsClient {
        fn new(cam_states: Vec<String>, publish_err: Option<NatsClientError>) -> Self {
            MockNatsClient {
                cam_states,
                publish_err,
            }
        }
    }

    impl INatsClient for MockNatsClient {
        fn publish(
            &mut self,
            _subject: &str,
            _payload: Option<&str>,
            _reply_subject: Option<&str>,
        ) -> Result<(), cliente::cliente::errors::NatsClientError> {
            if let Some(err) = self.publish_err.clone() {
                return Err(err);
            }
            Ok(())
        }

        fn hpublish(
            &mut self,
            _subject: &str,
            _headers: &str,
            _payload: &str,
            _reply_subject: Option<&str>,
        ) -> Result<(), Error> {
            todo!()
        }

        fn subscribe(
            &mut self,
            _subject: &str,
            f: Box<dyn Fn(SuscriptionInfo) + Send + 'static>,
        ) -> Result<usize, cliente::cliente::errors::NatsClientError> {
            for payload in &self.cam_states {
                thread::sleep(Duration::from_millis(100));
                f((payload.to_string(), None, None));
            }
            Ok(1)
        }

        fn unsubscribe(
            &mut self,
            _subject_id: usize,
            _max_msgs: Option<usize>,
        ) -> Result<(), cliente::cliente::errors::NatsClientError> {
            Ok(())
        }

        fn create_stream(&mut self, _subject: &str, _name: &str) -> Result<(), NatsClientError> {
            Ok(())
        }

        fn create_and_consume(
            &mut self,
            _stream_name: &str,
            _consumer_name: &str,
            _delivery_subject: &str,
            f: Box<dyn Fn(SuscriptionInfo) + Send + 'static>,
        ) -> Result<usize, NatsClientError> {
            for payload in &self.cam_states {
                thread::sleep(Duration::from_millis(100));
                f((payload.to_string(), None, None));
            }
            Ok(1)
        }
    }

    #[test]
    fn test01_nuevo_incidente_ok() {
        let config = Config::from_file("conf.json").unwrap();

        let mut sistema =
            SistemaDeMonitoreo::init(Box::new(MockNatsClient::new(vec![], None)), config).unwrap();
        let lat_esperada = 10.0;
        let long_esperada = 20.0;
        let tiempo_esperado = 30;

        let id = sistema
            .nuevo_incidente(lat_esperada, long_esperada, tiempo_esperado)
            .unwrap();

        let incidentes = sistema.obtener_info_incidentes().unwrap();

        assert_eq!(sistema.obtener_cant_de_incidentes().unwrap(), 1);
        assert_eq!(incidentes[0].get_id(), id);
        assert_eq!(incidentes[0].get_posision().lat, lat_esperada);
        assert_eq!(incidentes[0].get_posision().long, long_esperada);
    }

    #[test]
    fn test02_eliminar_incidente_ok() {
        let config = Config::from_file("conf.json").unwrap();

        let mut sistema =
            SistemaDeMonitoreo::init(Box::new(MockNatsClient::new(vec![], None)), config).unwrap();
        let id = sistema.nuevo_incidente(0.0, 0.0, 0).unwrap();

        sistema.eliminar_incidente(&id).unwrap();

        assert_eq!(sistema.obtener_cant_de_incidentes().unwrap(), 0);
    }

    #[test]
    fn test03_eliminar_incidente_id_inexistente() {
        let config = Config::from_file("conf.json").unwrap();

        let mut sistema =
            SistemaDeMonitoreo::init(Box::new(MockNatsClient::new(vec![], None)), config).unwrap();
        let id = "0";

        let invalid_id_error = sistema.eliminar_incidente(&id).unwrap_err();

        assert!(matches!(
            invalid_id_error,
            SistemaMonitoreoError::IdInvalido
        ));
    }

    #[test]
    fn test04_camara_creada_ok() {
        let config = Config::from_file("conf.json").unwrap();

        let lat = "12";
        let long = "21";
        let estado = "ahorro";
        let id = "0";
        let pos_cam = format!("{} {} {} {}", lat, long, estado, id);
        println!("AAAAAAAAA antes");
        let sistema =
            SistemaDeMonitoreo::init(Box::new(MockNatsClient::new(vec![pos_cam], None)), config)
                .unwrap();
        println!("AAAAAAAAA despues");
        let cams = sistema.obtener_info_camaras().unwrap();
        println!("AAAAAAAAA despues2");

        assert_eq!(cams.len(), 1);
        assert_eq!(cams[0].id, id);
        assert_eq!(cams[0].posicion.lat, lat.parse::<f64>().unwrap());
        assert_eq!(cams[0].posicion.long, long.parse::<f64>().unwrap());
        assert_eq!(cams[0].estado, estado);
    }

    #[test]
    fn test05_multiples_camaras_creadas_ok() {
        let config = Config::from_file("conf.json").unwrap();

        let sistema = SistemaDeMonitoreo::init(
            Box::new(MockNatsClient::new(
                vec!["0 21 ahorro 5".to_string(), "1 21 ahorro 25".to_string()],
                None,
            )),
            config,
        )
        .unwrap();

        let cams = sistema.obtener_info_camaras().unwrap();

        assert_eq!(cams.len(), 2);
    }

    #[test]
    fn test06_cambio_de_estado_camara_ok() {
        let config = Config::from_file("conf.json").unwrap();

        let sistema = SistemaDeMonitoreo::init(
            Box::new(MockNatsClient::new(
                vec!["0 21 ahorro 5".to_string(), "0 21 grabando 5".to_string()],
                None,
            )),
            config,
        )
        .unwrap();

        let cams = sistema.obtener_info_camaras().unwrap();

        assert_eq!(cams.len(), 1);
        assert_eq!(cams[0].estado, "grabando");
        assert_eq!(cams[0].id, "5");
        assert_eq!(cams[0].posicion.lat, 0.0);
        assert_eq!(cams[0].posicion.long, 21.0);
    }

    #[test]
    fn test06_cambio_de_estado_camara_a_inicial_ok() {
        let config = Config::from_file("conf.json").unwrap();

        let sistema = SistemaDeMonitoreo::init(
            Box::new(MockNatsClient::new(
                vec![
                    "0 21 ahorro 5".to_string(),
                    "0 21 grabando 5".to_string(),
                    "0 21 ahorro 5".to_string(),
                ],
                None,
            )),
            config,
        )
        .unwrap();

        let cams = sistema.obtener_info_camaras().unwrap();

        assert_eq!(cams.len(), 1);
        assert_eq!(cams[0].estado, "ahorro");
        assert_eq!(cams[0].id, "5");
        assert_eq!(cams[0].posicion.lat, 0.0);
        assert_eq!(cams[0].posicion.long, 21.0);
    }

    #[test]
    fn test07_eliminar_camaras_ok() {
        let config = Config::from_file("conf.json").unwrap();

        let sistema = SistemaDeMonitoreo::init(
            Box::new(MockNatsClient::new(
                vec![
                    "0 21 ahorro 1".to_string(),
                    "5 30 ahorro 2".to_string(),
                    "10 25 ahorro 3".to_string(),
                    "0 21 eliminado 1".to_string(),
                    "5 30 eliminado 2".to_string(),
                ],
                None,
            )),
            config,
        )
        .unwrap();

        let cams = sistema.obtener_info_camaras().unwrap();

        assert_eq!(cams.len(), 1);
        assert_eq!(cams[0].estado, "ahorro");
        assert_eq!(cams[0].id, "3");
        assert_eq!(cams[0].posicion.lat, 10.0);
        assert_eq!(cams[0].posicion.long, 25.0);
    }

    #[test]
    fn test08_falla_de_nuevo_incidente_controlada() {
        let config = Config::from_file("conf.json").unwrap();

        let nats = MockNatsClient::new(vec![], Some(NatsClientError::InvalidSubject));
        let mut sistema = SistemaDeMonitoreo::init(Box::new(nats), config).unwrap();

        let err = sistema.nuevo_incidente(0.0, 0.0, 0).unwrap_err();

        assert_eq!(sistema.obtener_cant_de_incidentes().unwrap(), 0);
        assert!(matches!(err, SistemaMonitoreoError::InternalError));
    }
}
