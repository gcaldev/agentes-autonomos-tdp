//mod estado_camara;
use crate::camara::estado_camara::EstadoCamara;
use crate::camara::posicion::Posicion;
use detector_incidentes::es_incidente;
use std::collections::HashMap;
use std::fs::{read_dir, remove_file, File, OpenOptions};
use std::io::{BufRead, BufReader, Error, Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};


#[derive(Debug)]
///Camara se inicia con estado AhorroDeEnergia y la posicion se pasa en el constrcutor.
pub struct Camara {
    pub id: usize,
    estado: EstadoCamara,
    posicion: Posicion,
    incidentes_id: Vec<i32>, //numero de vevtor iuguaL A CANT DE INCIDENTES 1QUE PUEDE GRABAR
    pub numlindantes: i32,   //numero de camaras de las que estan grabandoi y esta es lindante
    pub handler: Option<JoinHandle<()>>,
    tx: mpsc::Sender<()>,
    send_inc: mpsc::Sender<(f64, f64)>,
}

/// Lee de una carpeta
fn read_paths_from_folder(folder_path: &str) -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(entries) = read_dir(folder_path) {
        for entry in entries.flatten() {
            if let Some(path) = entry.path().to_str() {
                if !path.ends_with(".csv") {
                    paths.push(path.to_string());
                }
            }
        }
    }
    paths
}

/// Lee una imagen
fn read_image(path: &str) -> Result<Vec<u8>, Error> {
    let mut file = File::open(path)?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)?;
    Ok(buffer)
}


fn _read_incident_positions(path: &str) -> Result<HashMap<String, (f64, f64)>, Error> {//No la usamos: BORRAR
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut lines = HashMap::new();
    for line in reader.lines().map_while(Result::ok) {
        let parts: Vec<&str> = line.split(',').collect();
        println!("{:?} len: {}", parts, parts.len());
        if parts.len() >= 3 {
            if let Ok(lat) = parts[1].parse::<f64>() {
                if let Ok(long) = parts[2].parse::<f64>() {
                    lines.insert(parts[0].to_string(), (lat, long));
                    println!("Insertado: {},{}", lat, long);
                }
            }
        }
    }

    Ok(lines)
}

fn _update_file(updated_incidents: HashMap<String, (f64, f64)>, path: &str) -> Result<(), Error> {
    println!("{:?}", updated_incidents);
    let mut file = OpenOptions::new().write(true).truncate(true).open(path)?;

    for (file_name, (lat, long)) in updated_incidents.iter() {
        file.write_all(format!("{},{},{}", file_name, lat, long).as_bytes())?;
        println!("{},{},{}", file_name, lat, long);
        file.write_all(b"\n")?;
    }
    println!("Actualizado");
    Ok(())
}

//const RESOURCES_PATH: &str = "src/camara/resources";
const RESOURCES_PATH: &str = "../../camara/src/camara/resources";

/// Busca e informa si hay incidentes captados por la camara
pub fn buscar_posibles_incidentes(
    lat: f64,
    long: f64,  
    id: usize,
    rx: Receiver<()>,
    send_inc: Sender<(f64, f64)>,
) -> Option<JoinHandle<()>> {
    let builder = thread::Builder::new().name(format!("camara-thread-{}", id));

    if let Ok(handler) = builder
        .spawn(move || loop {
            println!("Antes del try recv");
            match rx.try_recv() {
                Ok(_) => break,
                Err(e) => {

                    println!("adentro pero puede haber error");
                    if e == mpsc::TryRecvError::Disconnected {
                        break;
                    }
                    println!("adentro pero ya no hay error");
                    println!("ID: {}", id);
                    let folder_path = format!("{}/{}", RESOURCES_PATH, id);
                    let images_path = read_paths_from_folder(&folder_path);

                    let images = images_path
                        .iter()
                        .fold(Vec::new(), |mut images, image_path| {
                            if let Ok(img_data) = read_image(image_path) {
                                images.push((image_path, img_data));
                                println!("Imagen leida");
                            }
                            images
                        });
                    
                    if images.is_empty(){
                        println!("Vector imagenes vacio");
                    }else{
                        println!("Vector imagenes tiene algo");
                    }

                    if let Some ((path, image)) = images.iter().next() {
                        if es_incidente(&image).unwrap_or(false) {
                            println!("Incidente registrado en {},{}", lat, long);
                            if let Err(e) = send_inc.send((lat+0.0005, long+0.0005)){
                                println!("Error en el thread de deteccion de imagenes {}", e);
                            }
                        }
                        if remove_file(path).is_err() {
                            println!("Error al borrar la imagen {}", path);
                        }
                    }
                    thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        }){
            Some(handler)
        }else{
            None
        }
        
    
}

impl Camara {
    // Retorna una instancia de Camaras con las suscripciones ya hechas
    pub fn new(
        id: usize,
        longitud: f64,
        latitud: f64,
        send_inc: Sender<(f64, f64)>,
    ) -> Result<Self, Error> {
        let (tx, rx) = mpsc::channel::<()>();

        Ok(Camara {
            id,
            estado: EstadoCamara::AhorroDeEnergia,
            posicion: Posicion {
                lat: latitud,
                long: longitud,
            },
            incidentes_id: Vec::new(),
            numlindantes: 0,
            handler: buscar_posibles_incidentes(latitud,longitud,id, rx, send_inc.clone()),
            tx,
            send_inc,
        })
    }

    pub fn set_longitud(&mut self, longitud: f64) {
        self.posicion.long = longitud;
    }

    pub fn set_latitud(&mut self, latitud: f64) {
        self.posicion.long = latitud;
    }

    pub fn agregar_incidente(&mut self, id: i32) {
        self.incidentes_id.push(id);
    }

    pub fn quitar_incidente(&mut self, id: i32) {
        let mut indice = 0;
        let mut matcheo = false;

        for inc in &self.incidentes_id {
            if inc == &id {
                matcheo = true;
            }

            if !matcheo {
                indice += 1;
            }
        }
        self.incidentes_id.remove(indice);
    }

    pub fn get_cantidad_incidentes(&self) -> usize {
        self.incidentes_id.len()
    }

    pub fn contiene_incidente(&self, id: i32) -> bool {
        self.incidentes_id.contains(&id)
    }

    pub fn set_estado_grabar(&mut self) {
        self.estado = EstadoCamara::Grabando;
        if let Err(e) = self.tx.send(()){
            println!("Error al enviar mensaje {}", e);
        }
        if let Some(handler) = self.handler.take() {
            if let Err(_e) = handler.join(){
                println!("Error al hacer handle join");
            }
        }
    }

    pub fn set_estado_ahorro_de_energia(&mut self) {
        self.estado = EstadoCamara::AhorroDeEnergia;
        let (tx, rx) = mpsc::channel::<()>();
        self.tx = tx;
        self.handler = buscar_posibles_incidentes(
            self.get_posision().lat,
            self.get_posision().long,
            self.id,
            rx,
            self.send_inc.clone(),
        );
    }

    pub fn get_estado(&self) -> EstadoCamara {
        self.estado.clone()
    }

    pub fn get_posision(&self) -> Posicion {
        self.posicion.clone()
    }

    pub const fn get_id(&self) -> &usize {
        &self.id
    }
}
