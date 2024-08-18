//App que cundo es instanciada se queda funcionando para procesar los mensajes que escuche
//de los canales a los que esta suscrito.
//Y procesar algunos mensajes implica publicar en otros canalaes.

use super::camara_dto::CamaraDto;
use super::errores::ErroresCentral;
#[allow(unused_imports)]
use super::estado_camara::EstadoCamara;
use crate::camara::camara::Camara;
use crate::camara::config::Config;
use crate::camara::posicion::Posicion;
use cliente::cliente::client::NatsClient;
use cliente::cliente::iclient::INatsClient;
use cliente::cliente::user::User;
use std::collections::HashMap;
use std::io::Error;
use std::net::TcpStream;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;

/// Permite realizar las operaciones asociadas a las camaras
pub struct CentralDeCamaras {
    pub lista_de_camaras: Arc<Mutex<HashMap<usize, Camara>>>,
    pub client: Arc<Mutex<Box<dyn INatsClient + Send>>>,
    id: usize,
    pub config: Config,
    pub incidentes: Arc<Mutex<HashMap<(i32, String), String>>>,
    send_inc: Sender<(f64, f64)>,
}

/// Parsea un mensaje de incidente esperando  id, latitud, longitud y estado separados por espacios
fn parse_incident(payload: &str) -> Result<(i32, f64, f64, &str), ErroresCentral> {
    println!("Parseando incidente: {}", payload);
    let mut iter = payload.split_whitespace();
    if let Some(id) = iter.next() {
        if let Ok(id) = id.parse::<i32>() {
            if let Some(latitud) = iter.next() {
                if let Ok(latitud) = latitud.parse::<f64>() {
                    if let Some(longitud) = iter.next() {
                        if let Ok(longitud) = longitud.parse::<f64>() {
                            if let Some(estado) = iter.next() {
                                println!(
                                    "Id: {} Latitud: {} Longitud: {} Estado: {}",
                                    id, latitud, longitud, estado
                                );
                                return Ok((id, latitud, longitud, estado));
                            }
                        }
                    }
                }
            }
        }
    }
    Err(ErroresCentral::ErrorAlProcesarIncidente)
}

/// Procesa un nuevo incidente 
fn process_incidents(
    payload: (String, Option<String>, Option<String>),
    lista_lock: &Arc<Mutex<HashMap<usize, Camara>>>,
    nats_lockeado_inc: &Arc<Mutex<Box<dyn INatsClient + Send>>>,
    pub_camara: String,
    areaincidente: f64,
    arealindante: f64,
    incidentes_lock: &Arc<Mutex<HashMap<(i32, String), String>>>,
) -> Result<(), ErroresCentral> {
    let (id, latitud, longitud, estado) = parse_incident(&payload.0)?;
    if let Ok(mut incidentes_guardados) = incidentes_lock.lock() {
        if incidentes_guardados.contains_key(&(id, estado.to_string())) || estado == "activo"{
            return Ok(());
        } else {
            incidentes_guardados.insert((id, estado.to_string()), "_".to_string());
        }
    }

    if let Ok(mut lista_cam) = lista_lock.lock() {
        let mut ids_cams_en_rango: HashMap<usize, usize> = HashMap::new();
        let lista_cams_en_rango = lista_cam.iter_mut().filter(|(_, cam)| {
            cam.get_posision()
                .en_rango(latitud, longitud, areaincidente)
        });

        if estado == "pendiente" {
            for (id_cam, cam) in lista_cams_en_rango {
                if !ids_cams_en_rango.contains_key(id_cam) {
                    ids_cams_en_rango.insert(*id_cam, 0);
                }
                if !cam.contiene_incidente(id) {
                    cam.agregar_incidente(id);
                    cam.numlindantes -= 1;
                    if cam.get_estado() == EstadoCamara::AhorroDeEnergia {
                        if let Ok(mut client) = nats_lockeado_inc.lock() {
                            cam.set_estado_grabar();
                            let Posicion { lat, long } = cam.get_posision();
                            let payload =
                                format!("{} {} {} {}", lat, long, "grabando", cam.get_id());
                            if client.publish(&pub_camara, Some(&payload), None).is_err() {
                                println!("Error Interno: Al hacer publish de camara");
                            }
                        }
                    }
                }
            }

            for id_cam in ids_cams_en_rango.keys() {
                if let Some(camara) = lista_cam.get_mut(id_cam) {
                    let pos_cam_actual = camara.get_posision();
                    let lista_cams_lindantes = lista_cam.iter_mut().filter(|(_, cam)| {
                        cam.get_posision().en_rango(
                            pos_cam_actual.lat,
                            pos_cam_actual.long,
                            arealindante,
                        )
                    });
                    for (_id_lindante, cam_lindante) in lista_cams_lindantes {
                        cam_lindante.numlindantes += 1;
                        if cam_lindante.get_estado() == EstadoCamara::AhorroDeEnergia {
                            if let Ok(mut client) = nats_lockeado_inc.lock() {
                                cam_lindante.set_estado_grabar();
                                let Posicion { lat, long } = cam_lindante.get_posision();
                                let payload = format!(
                                    "{} {} {} {}",
                                    lat,
                                    long,
                                    "grabando",
                                    cam_lindante.get_id()
                                );
                                if client.publish(&pub_camara, Some(&payload), None).is_err() {
                                    println!("Error Interno: Al hacer publish de camara");
                                }
                            }
                        }
                    }
                }
            }
        } else if estado == "cancelado" {
            if let Ok(mut client) = nats_lockeado_inc.lock() {
                for (id_cam, cam) in lista_cams_en_rango {
                    if !ids_cams_en_rango.contains_key(id_cam) {
                        ids_cams_en_rango.insert(*id_cam, 0);
                    }

                    cam.numlindantes += 1;
                    if cam.contiene_incidente(id) {
                        cam.quitar_incidente(id);
                        if cam.get_cantidad_incidentes() == 0 && cam.numlindantes == 0 {
                            cam.set_estado_ahorro_de_energia();
                            let Posicion { lat, long } = cam.get_posision();
                            let payload = format!("{} {} {} {}", lat, long, "ahorro", cam.get_id());
                            if client.publish(&pub_camara, Some(&payload), None).is_err() {
                                println!("Error Interno: Al hacer publish de camara");
                            }
                        }
                    }
                }
            } else {
                println!("Error Interno: Lock envenenado");
            }

            for id_cam in ids_cams_en_rango.keys() {
                if let Some(camara) = lista_cam.get_mut(id_cam) {
                    let pos_cam_actual = camara.get_posision();
                    avisa_lindantes_incidente_cancelado(
                        &mut lista_cam,
                        arealindante,
                        pos_cam_actual,
                        nats_lockeado_inc,
                        &pub_camara,
                    );
                }
            }
        }
    } else {
        println!("Error Interno: Lock envenenado");
    }
    Ok(())
}

///camara avisa a sus lindantes que incidente es cancelado:
pub fn avisa_lindantes_incidente_cancelado(
    lista_cam: &mut HashMap<usize, Camara>,
    arealindante: f64,
    pos_cam_actual: Posicion,
    nats_lockeado_inc: &Arc<Mutex<Box<dyn INatsClient + Send>>>,
    pub_camara: &str,
) {
    if let Ok(mut client) = nats_lockeado_inc.lock() {
        let lista_cams_lindantes = lista_cam.iter_mut().filter(|(_, cam)| {
            cam.get_posision()
                .en_rango(pos_cam_actual.lat, pos_cam_actual.long, arealindante)
        });

        for (_id, camlindante) in lista_cams_lindantes {
            camlindante.numlindantes -= 1;
            if camlindante.get_cantidad_incidentes() == 0 && camlindante.numlindantes == 0 {
                camlindante.set_estado_ahorro_de_energia();
                let Posicion { lat, long } = camlindante.get_posision();
                let payload = format!("{} {} {} {}", lat, long, "ahorro", camlindante.get_id());
                if client.publish(pub_camara, Some(&payload), None).is_err() {
                    println!("Error Interno: Al hacer publish de camara");
                }
            }
        }
    } else {
        println!("Error Interno: Lock envenenado");
    }
}

impl CentralDeCamaras {
    /// Función de inicialización que configura las suscripciones NATS
    fn init(nats: Box<dyn INatsClient + Send>, config: Config) -> Result<Self, Error> {
        let lista: Arc<Mutex<HashMap<usize, Camara>>> = Arc::new(Mutex::new(HashMap::new()));
        let lista_lock = Arc::clone(&lista);
        let incidentes: Arc<Mutex<HashMap<(i32, String), String>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let incidentes_lock: Arc<Mutex<HashMap<(i32, String), String>>> = Arc::clone(&incidentes);

        let nats_lockeado = Arc::new(Mutex::new(nats));
        let nats_lockeado_inc = Arc::clone(&nats_lockeado);
        let nats_lockeado_save = Arc::clone(&nats_lockeado);

        let pub_camara = config.pub_camara.clone();
        let areaincidente = config.areaincidente;
        let arealindante = config.arealindante;

        let mut nats_safe = nats_lockeado
            .lock()
            .map_err(|_| ErroresCentral::InternalError("Lock envenenado".to_string()))?;

        let subincidentes = &config.sub_incidentes;
        let jetstream_name = &format!("JetStream{}", subincidentes);
        let consumer_name = &format!("Central_Camaras_Consumer{}", subincidentes);
        let delivery_subject = &format!("Central_Camaras_Delivery{}", subincidentes);
        nats_safe.create_stream(&config.sub_incidentes, jetstream_name)?;

        nats_safe.create_and_consume(
            jetstream_name,
            consumer_name,
            delivery_subject,
            Box::new(move |payload| {
                if let Err(e) = process_incidents(
                    payload,
                    &lista_lock,
                    &nats_lockeado_inc,
                    pub_camara.clone(),
                    areaincidente,
                    arealindante,
                    &incidentes_lock,
                ) {
                    println!("Error al procesar incidente: {}", e);
                }
            }),
        )?;
        drop(nats_safe);

        let (send_inc, receive_inc) = std::sync::mpsc::channel::<(f64, f64)>();

        let builder = thread::Builder::new().name("notify-incidents-thread".to_string());        
        let _inc_ref = incidentes.clone();
        let pub_incidentes_ia_th = config.pub_incidentes_ia.to_string();
        
        if let Err(e) = builder
            .spawn(move || {
                loop {
                    println!("Empiezo a esperar si hay algun mensaje de que se detecto una imagen");
                    match receive_inc.recv() {
                        Ok((lat,long)) => {                        
                            if let Ok(mut nats) = nats_lockeado.lock(){
                                let payload = format!("{} {}", lat, long);                            
                                if let Err(e)=nats.publish(&pub_incidentes_ia_th, Some(&payload), None){
                                    println!("{}", e);
                                }
                                println!("Detecto incidente desde Camara______________IA");
                            }
                        }
                    Err(_) => {
                        break
                    }   
                }
            }}){
                println!("Error {}", e);
                return Err(e);
            }

        Ok(CentralDeCamaras {
            lista_de_camaras: lista,
            client: nats_lockeado_save,
            id: 0usize,
            config,
            incidentes,
            send_inc
        })
        
    }

    /// Provee el estado actual de las camaras
    pub fn obtener_info_camaras(&self) -> Result<Vec<CamaraDto>, ErroresCentral> {
        self.lista_de_camaras.lock().map_or_else(|_| Err(ErroresCentral::InternalError("Lock envenenado".to_string())), |lista| {
            let mut camaras: Vec<CamaraDto> = lista.iter().map(|(_, camara)| CamaraDto::from(camara)).collect();
            camaras.sort_by_key(|camara| camara.id);
            Ok(camaras)
        })
    }

    ///Inicializa app
    pub fn new() -> Result<Self, Error> {
        let config = match Config::from_file("conf.json") {
            Ok(config) => config,
            Err(e) => {
                println!("Error al cargar conf.json: {}", e);
                return Err(e);
            }
        };
        let writer = TcpStream::connect(&config.address_client)?; // Conecta el escritor al servidor NATS p[ara escribir en channel a Camara
        let reader = writer.try_clone()?; // Clona el escritor para usarlo como lector de Camara y de Sist de Monitoreo

        //nuevo cliente NATS
        let nats = NatsClient::new(
            writer,
            reader,
            "logs.txt",
            Some(User::new(config.username.clone(), config.password.clone())),
        )?;

        Self::init(Box::new(nats), config) // Inicializa el sistema de monitoreo con el cliente NATS
    }

    ///Levanta una app Camara en caso no exista ya una con ese ID.
    pub fn nueva_camara(&mut self, lat: f64, long: f64) -> Result<&usize, ErroresCentral> {
        self.id += 1;
        if let Ok(cam) = Camara::new(self.id, long, lat,self.send_inc.clone()) {
            if let Ok(mut lista) = self.lista_de_camaras.lock() {
                lista.insert(self.id, cam);
                println!("Agrego nueva camara"); //dsp quitar
                publish_camara(
                    & self.client,
                    lat,
                    long,
                    "ahorro",
                    &self.id,
                    &self.config.pub_camara,
                )?;

                Ok(&self.id)
            } else {
                Err(ErroresCentral::InternalError("Lock envenenado".to_string()))
            }
        } else {
            self.id -= 1;
            Err(ErroresCentral::ErrorAlCrearCamara)
        }
    }

    #[allow(clippy::significant_drop_in_scrutinee)]
    /// Elimina una camara de la central
    pub fn eliminar_camara(&mut self, id: &usize) -> Result<(), ErroresCentral> {
        if let Ok(mut lista) = self.lista_de_camaras.lock() {
            let config = match Config::from_file("conf.json") {
                Ok(config) => config,
                Err(e) => {
                    println!("Error al cargar conf.json: {}", e);
                    return Err(ErroresCentral::NoSePudoBorrarLaCamara);
                }
            };

            let camara: Option<&Camara> = lista.get(id);
            
            let (cantidad_incidentes, pos) = match camara {
                Some(c) => {
                    let cant_incidentes = c.get_cantidad_incidentes();
                    (cant_incidentes, c.get_posision())
                }
                None => {
                    println!("No se encontró la cámara con ese id.");
                    return Err(ErroresCentral::NoSePudoBorrarLaCamara);
                }
            };

            for _ in 0..cantidad_incidentes {
                avisa_lindantes_incidente_cancelado(
                    &mut lista,
                    config.arealindante,
                    pos.clone(),
                    &self.client,
                    &config.pub_camara,
                );
            }

            match lista.remove(id) {
                Some(_) => {
                    println!("Quito camara"); //dsp quitar
                    publish_camara(
                        & self.client,
                        0.0,
                        0.0,
                        "eliminado",
                        id,
                        &self.config.pub_camara,
                    )?;
                    Ok(())
                }
                None => Err(ErroresCentral::NoSePudoBorrarLaCamara),
            }
        } else {
            Err(ErroresCentral::InternalError("Lock envenenado".to_string()))
        }
    }
}

/// Publica informacion de la camara en el servidor
fn publish_camara(
    client_lockeado: &Arc<Mutex<Box<dyn INatsClient + Send>>>,
    lat: f64,
    long: f64,
    estado: &str,
    id: &usize,
    pub_camara: &str,
) -> Result<(), ErroresCentral> {
    if let Ok(mut client) = client_lockeado.lock() {
        let payload = format!(
            "{} {} {} {}",
            lat,
            long,
            estado,
            id,
        );

        client
            .publish(pub_camara, Some(&payload), None)
            .map_err(|_| ErroresCentral::InternalError("Al hacer publish de camara".to_string()))?;
        Ok(())
    } else {
        Err(ErroresCentral::InternalError("Lock envenenado".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use std::{thread, time::Duration};

    use cliente::cliente::{errors::NatsClientError, suscriptor_handler::SuscriptionInfo};

    use super::*;

    struct MockNatsClient {
        publish_err: Option<NatsClientError>,
        lista_fn: Arc<Mutex<Vec<Box<dyn Fn(SuscriptionInfo) + Send + 'static>>>>,
    }

    impl MockNatsClient {
        fn new(publish_err: Option<NatsClientError>) -> Self {
            MockNatsClient {
                publish_err,
                lista_fn: Arc::new(Mutex::new(vec![])),
            }
        }

        fn get_sub_trigger(&mut self) -> Box<dyn Fn(&str)> {
            let (tx, rx) = std::sync::mpsc::channel::<String>();
            let lista_fn_clone = self.lista_fn.clone();

            thread::spawn(move || loop {
                match rx.recv() {
                    Ok(msg) => {
                        let lista_fn = lista_fn_clone.lock().unwrap();
                        let f = &lista_fn[0];
                        f((msg.clone(), None, None));
                    }
                    Err(_) => break,
                }
            });

            Box::new(move |payload| {
                tx.send(payload.to_string()).unwrap();
                thread::sleep(Duration::from_millis(100));
            })
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
            let mut lista_fn = self.lista_fn.lock().unwrap();
            lista_fn.push(f);
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
            let mut lista_fn = self.lista_fn.lock().unwrap();
            lista_fn.push(f);
            Ok(1)
        }
    }

    #[test]
    fn test01_nueva_camara() {
        let config = Config::from_file("conf.json").unwrap();

        let nats = MockNatsClient::new(None);
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        central.nueva_camara(1.0, 1.0).unwrap();

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 1);
        assert_eq!(camaras[0].get_posision().lat, 1.0);
        assert_eq!(camaras[0].get_posision().long, 1.0);
    }

    #[test]
    fn test02_error_al_publicar_creacion_camara() {
        let config = Config::from_file("conf.json").unwrap();

        let nats = MockNatsClient::new(Some(NatsClientError::InternalError("Error".to_string())));
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        let res = central.nueva_camara(1.0, 1.0).unwrap_err();
        assert!(matches!(res, ErroresCentral::InternalError(_)));
    }

    #[test]
    fn test03_muchas_camaras_nuevas_ok() {
        let config = Config::from_file("conf.json").unwrap();

        let nats = MockNatsClient::new(None);
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        central.nueva_camara(1.0, 1.0).unwrap();
        central.nueva_camara(2.0, 2.0).unwrap();

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 2);
    }

    #[test]
    fn test04_incidente_lejos_no_cambia_estado() {
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let trigger_sub = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();
        central.nueva_camara(1.0, 1.0).unwrap();

        trigger_sub("0 50000 500001 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 1);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::AhorroDeEnergia);
    }

    #[test]
    fn test05_incidente_cerca_cambia_estado() {
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let trigger_sub = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();
        central.nueva_camara(1.0, 1.0).unwrap();

        trigger_sub("0 1 1 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 1);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
    }

    #[test]
    fn test06_incidente_cancelado_cambia_estado_ahorro() {
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();
        central.nueva_camara(1.0, 1.0).unwrap();
        enviar_incidente("0 1 1 pendiente");

        enviar_incidente("0 1 1 cancelado");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 1);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::AhorroDeEnergia);
    }

    #[test]
    fn test07_incidentes_uno_cancelado_uno_pendiente_mantiene_estado_grabando() {
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();
        central.nueva_camara(1.0, 1.0).unwrap();

        enviar_incidente("0 1 1 pendiente");
        enviar_incidente("1 2 2 pendiente");
        enviar_incidente("0 1 1 cancelado");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 1);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
    }
    #[test]
    fn test08_eliminar_camara_existente() {
        let config = Config::from_file("conf.json").unwrap();

        let nats = MockNatsClient::new(None);
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();
        central.nueva_camara(1.0, 1.0).unwrap();

        central.eliminar_camara(&1).unwrap();

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 0);
    }

    #[test]
    fn test09_eliminar_camara_inexistente() {
        let config = Config::from_file("conf.json").unwrap();

        let nats = MockNatsClient::new(None);
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();
        central.nueva_camara(1.0, 1.0).unwrap();

        let err = central.eliminar_camara(&2).unwrap_err();

        assert!(matches!(err, ErroresCentral::NoSePudoBorrarLaCamara));
    }

    //testes lindantes:
    //Ojo, esto funciona solo si la config es: area incidente:1.1  area lindnates: 3.1
    #[test]
    fn test10_camara_en_rango_incidente_se_activa_y_sus_lindantes() {
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let trigger_sub = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        //camara en area de incidente
        central.nueva_camara(2.0, 1.0).unwrap();

        //camaras en area lindnate a camara con incidente
        central.nueva_camara(3.0, 1.0).unwrap();
        central.nueva_camara(4.0, 1.0).unwrap();
        central.nueva_camara(5.0, 2.0).unwrap();

        //incidente
        trigger_sub("0 1 1 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 4);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::Grabando);
    }

    #[test]
    fn test11_camara_en_rango_incidente_y_sus_lindantes_al_eliminar_incidente_toda_pasan_ahorro_de_energia(
    ) {
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        //camara en area de incidente
        central.nueva_camara(2.0, 1.0).unwrap(); //ara incidente:1.1  area lindnates: 3.1

        //camaras en area lindnate a camara con incidente
        central.nueva_camara(3.0, 1.0).unwrap();
        central.nueva_camara(4.0, 1.0).unwrap();
        central.nueva_camara(5.0, 2.0).unwrap();

        //incidente
        enviar_incidente("0 1 1 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 4);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::Grabando);

        enviar_incidente("0 1 1 cancelado");
        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 4);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::AhorroDeEnergia);
    }

    #[test]
    fn test12_2_incidenetes_al_aliminar_uno_en_misma_posicion_camaras_y_lindantes_se_mantienen_grabando(
    ) {
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        //camara en area de incidente
        central.nueva_camara(2.0, 1.0).unwrap(); //ara incidente:1.1  area lindnates: 3.1

        //camaras en area lindnate a camara con incidente
        central.nueva_camara(3.0, 1.0).unwrap();
        central.nueva_camara(4.0, 1.0).unwrap();
        central.nueva_camara(5.0, 2.0).unwrap();

        //incidente
        enviar_incidente("0 1 1 pendiente");
        enviar_incidente("1 1 1 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 4);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::Grabando);

        enviar_incidente("0 1 1 cancelado");
        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 4);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::Grabando);
    }

    #[test]
    fn test13_muchos_incidenetes_todos_eliminados_todas_las_camaras_en_ahorro_de_energia() {
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        //camara en area de incidente
        central.nueva_camara(2.0, 1.0).unwrap();
        //camaras en area lindnate a camara con incidente
        central.nueva_camara(3.0, 1.0).unwrap();
        central.nueva_camara(4.0, 1.0).unwrap();
        central.nueva_camara(5.0, 2.0).unwrap();

        //incidente
        enviar_incidente("0 1 1 pendiente");
        enviar_incidente("1 1 1 pendiente");
        enviar_incidente("2 1 1 pendiente");
        enviar_incidente("3 1 1 pendiente");
        enviar_incidente("4 1 1 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 4);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::Grabando);

        enviar_incidente("0 1 1 cancelado");
        enviar_incidente("1 1 1 cancelado");
        enviar_incidente("2 1 1 cancelado");
        enviar_incidente("3 1 1 cancelado");
        enviar_incidente("4 1 1 cancelado");
        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 4);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::AhorroDeEnergia);
    }

    #[test]
    fn test14_muchos_incidenetes_todos_eliminados_menos_1_camaras_siguen_grabando() {
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        //camara en area de incidente
        central.nueva_camara(2.0, 1.0).unwrap();

        //camaras en area lindnate a camara con incidente
        central.nueva_camara(3.0, 1.0).unwrap();
        central.nueva_camara(4.0, 1.0).unwrap();
        central.nueva_camara(5.0, 2.0).unwrap();

        //incidente
        enviar_incidente("0 1 1 pendiente");
        enviar_incidente("1 1 1 pendiente");
        enviar_incidente("2 1 1 pendiente");
        enviar_incidente("3 1 1 pendiente");
        enviar_incidente("4 1 1 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 4);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::Grabando);

        enviar_incidente("0 1 1 cancelado");
        enviar_incidente("1 1 1 cancelado");
        enviar_incidente("2 1 1 cancelado");
        enviar_incidente("3 1 1 cancelado");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 4);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::Grabando);
    }

    #[test]
    fn test15_camaras_fuera_del_rango_de_incidente_y_fuera_de_rango_de_ser_lindantes_de_las_que_tienen_incidente_no_graban(
    ) {
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        //camara en area de incidente
        central.nueva_camara(2.0, 1.0).unwrap();

        //camaras fuera del rango de lindantes de la que tiene un incidente
        central.nueva_camara(8.0, 8.0).unwrap();
        central.nueva_camara(7.0, 7.0).unwrap();

        //incidente
        enviar_incidente("0 1 1 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 3);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::AhorroDeEnergia);
    }

    #[test]
    fn test16_camaras_fuera_del_rango_de_incidente_y_fuera_de_rango_de_ser_lindantes_de_las_que_tienen_incidente_no_graban_al_ser_incdente_eliminado_todas_no_graban(
    ) {
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        //camara en area de incidente
        central.nueva_camara(2.0, 1.0).unwrap();

        //camaras fuea del rango de lindantes de laque tiene un incidente
        central.nueva_camara(8.0, 8.0).unwrap();
        central.nueva_camara(7.0, 7.0).unwrap();

        //incidente
        enviar_incidente("0 1 1 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 3);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::AhorroDeEnergia);

        enviar_incidente("0 1 1 cancelado");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 3);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[2].get_estado(), EstadoCamara::AhorroDeEnergia);
    }

    #[test]
    fn test17_camaras_lindantes_graban_por_dos_camaras_distintas_con_incidentes_distintos() {
        //cam_con_incidente cam_lindante cam_lindante2 cam_con_otro_incidente
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        //camara en area de incidente
        central.nueva_camara(2.0, 1.0).unwrap();

        //camara enb area de un incidente diferente
        central.nueva_camara(8.0, 8.0).unwrap();

        //camaras en area lindnate a camara con incidente
        central.nueva_camara(3.0, 1.0).unwrap();
        central.nueva_camara(4.0, 4.0).unwrap();
        central.nueva_camara(5.0, 5.0).unwrap();
        central.nueva_camara(6.0, 6.0).unwrap();
        central.nueva_camara(7.0, 7.0).unwrap();

        //incidente
        enviar_incidente("0 1 1 pendiente");
        enviar_incidente("1 9 9 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 7);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);

        assert_eq!(camaras[2].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[4].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[5].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[6].get_estado(), EstadoCamara::Grabando);
    }

    #[test]
    fn test18_camaras_lindantes_graban_por_dos_camaras_distintas_con_incidentes_distintos_al_eliminar_incidente_de_un_extremo_manitnene_lindante_por_la_otra_cam_con_incidente(
    ) {
        //cam_con_incidente cam_lindante cam_lindante2 cam_con_otro_incidente
        let config = Config::from_file("conf.json").unwrap();

        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        //camara en area de incidente
        central.nueva_camara(2.0, 1.0).unwrap();

        //camara enb area de un incidente diferente
        central.nueva_camara(8.0, 8.0).unwrap();

        //camaras en area lindnate a camara con incidente
        central.nueva_camara(3.0, 1.0).unwrap();
        central.nueva_camara(4.0, 4.0).unwrap();
        central.nueva_camara(5.0, 5.0).unwrap();
        central.nueva_camara(6.0, 6.0).unwrap();
        central.nueva_camara(7.0, 7.0).unwrap();

        //incidente
        enviar_incidente("0 1 1 pendiente");
        enviar_incidente("1 9 9 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 7);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);

        assert_eq!(camaras[2].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[4].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[5].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[6].get_estado(), EstadoCamara::Grabando);

        enviar_incidente("0 1 1 cancelado");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 7);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);

        assert_eq!(camaras[2].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[4].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[5].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[6].get_estado(), EstadoCamara::Grabando);
    }
    #[test]
    fn test19_dos_camaras_en_extremos_con_incdente_una_deja_de_tener_incidente_y_alcanza_a_la_que_tiene_al_limite_de_lindante(
    ) {
        //cam_con_incidente cam_lindante cam_lindante2 cam_con_otro_incidente
        let config = Config::from_file("conf.json").unwrap();
        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        //camara en area de incidente
        central.nueva_camara(2.0, 2.0).unwrap();

        //camara enb area de un incidente diferente
        central.nueva_camara(8.0, 8.0).unwrap();

        //camaras en area lindnate a camara con incidente
        central.nueva_camara(3.0, 3.0).unwrap();
        central.nueva_camara(4.0, 4.0).unwrap();
        central.nueva_camara(5.0, 5.0).unwrap();
        central.nueva_camara(6.0, 6.0).unwrap();
        central.nueva_camara(7.0, 7.0).unwrap();

        //incidente
        enviar_incidente("0 1 1 pendiente");
        enviar_incidente("1 9 9 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 7);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);

        assert_eq!(camaras[2].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[4].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[5].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[6].get_estado(), EstadoCamara::Grabando);

        enviar_incidente("0 1 1 cancelado");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 7);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);

        assert_eq!(camaras[2].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[4].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[5].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[6].get_estado(), EstadoCamara::Grabando);
    }
    #[test]
    fn test20_camara_es_lindante_a_dos_camaras_con_incidente_y_pasa_a_estado_ahorro_cuando_ambas_dejan_de_tener_incidente(
    ) {
        let config = Config::from_file("conf.json").unwrap();
        let mut nats = MockNatsClient::new(None);
        let enviar_incidente = nats.get_sub_trigger();
        let mut central = CentralDeCamaras::init(Box::new(nats), config).unwrap();

        //camara en area de incidente
        central.nueva_camara(2.0, 2.0).unwrap();

        //camara enb area de un incidente diferente
        central.nueva_camara(8.0, 8.0).unwrap();

        //camaras en area lindnate a camara con incidente
        central.nueva_camara(3.0, 3.0).unwrap();
        central.nueva_camara(4.0, 4.0).unwrap();
        central.nueva_camara(5.0, 5.0).unwrap();
        central.nueva_camara(6.0, 6.0).unwrap();
        central.nueva_camara(7.0, 7.0).unwrap();

        //incidente
        enviar_incidente("0 1 1 pendiente");
        enviar_incidente("1 9 9 pendiente");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 7);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);

        assert_eq!(camaras[2].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[4].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[5].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[6].get_estado(), EstadoCamara::Grabando);

        enviar_incidente("0 1 1 cancelado");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 7);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::Grabando);

        assert_eq!(camaras[2].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[4].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[5].get_estado(), EstadoCamara::Grabando);
        assert_eq!(camaras[6].get_estado(), EstadoCamara::Grabando);

        enviar_incidente("1 9 9 cancelado");

        let camaras = central.obtener_info_camaras().unwrap();
        assert_eq!(camaras.len(), 7);
        assert_eq!(camaras[0].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[1].get_estado(), EstadoCamara::AhorroDeEnergia);

        assert_eq!(camaras[2].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[3].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[4].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[5].get_estado(), EstadoCamara::AhorroDeEnergia);
        assert_eq!(camaras[6].get_estado(), EstadoCamara::AhorroDeEnergia);
    }
}
