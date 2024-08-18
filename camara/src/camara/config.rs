use serde::Deserialize;
use std::fs::File;
use std::io::BufReader;
use std::io::Error;

#[derive(Deserialize, Debug)]
/// Encargado de proveer la configuración de la central de camaras
pub struct Config {
    pub address_client: String,
    pub areaincidente: f64,
    pub arealindante: f64,
    pub pub_camara: String,
    pub sub_incidentes: String,
    pub pub_incidentes_ia: String,
    pub username: String,
    pub password: String,
}

impl Config {
    pub fn from_file(file_path: &str) -> Result<Self, Error> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);
        let config = serde_json::from_reader(reader)?;
        Ok(config)
    }
}
