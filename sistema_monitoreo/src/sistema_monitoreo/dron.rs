use super::posicion::Posicion;

#[derive(Debug, Clone)]
/// Representa un dron, guardando los datos recibidos por los drones
pub struct Dron {
    pub id: String,
    pub estado: String,
    pub posicion: Posicion,
    pub bateria: f64,
}

impl Dron {
    pub const fn new(id: String, posicion: Posicion, estado: String, bateria: f64) -> Self {
        Dron {
            id,
            estado,
            posicion,
            bateria,
        }
    }

    pub fn set_estado(&mut self, estado: String) {
        self.estado = estado;
    }

    pub fn set_posicion(&mut self, latitud: f64, longitud: f64) {
        self.posicion.set(latitud, longitud);
    }

    pub fn set_bateria(&mut self, bateria: f64) {
        self.bateria = bateria;
    }
}
