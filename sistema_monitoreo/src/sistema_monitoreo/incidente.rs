use crate::sistema_monitoreo::estado::Estado;
use crate::sistema_monitoreo::posicion::Posicion;

/// Cada incidente es creado con estado pendiente.
#[derive(Debug, Clone)]
pub struct Incidente {
    id: String,
    tiempo_de_resolcuion: u64,
    posicion: Posicion,
    estado: Estado,
}

impl Incidente {
    /// Crea un nuevo incidente
    pub fn new(tiempo_de_resolucion: u64, lat: f64, long: f64, id: &str) -> Self {
        Incidente {
            id: id.to_string(),
            tiempo_de_resolcuion: tiempo_de_resolucion,
            posicion: Posicion::new(lat, long),
            estado: Estado::Pendiente,
        }
    }

    pub fn get_posision(&self) -> Posicion {
        self.posicion.clone()
    }

    pub fn get_id(&self) -> &str {
        &self.id
    }

    pub const fn get_tiempo_resolucion(&self) -> u64 {
        self.tiempo_de_resolcuion
    }

    pub const fn get_estado(&self) -> &Estado {
        &self.estado
    }

    pub fn set_estado(&mut self, estado: Estado) {
        self.estado = estado;
    }
}
