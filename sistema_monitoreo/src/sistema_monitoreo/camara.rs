use super::posicion::Posicion;

#[derive(Debug, Clone)]
/// Representa una camara, guarda los datos recibidos por la central de camaras
pub struct Camara {
    pub id: String,
    pub estado: String,
    pub posicion: Posicion,
}

impl Camara {
    pub const fn new(id: String, posicion: Posicion, estado: String) -> Self {
        Camara {
            id,
            estado, // DEBERIA MANDAR EL ESTADO INICIAL LA CENTRAL DE CAMARAS
            posicion,
        }
    }

    pub fn cambiar_estado(&mut self, estado: String) {
        self.estado = estado;
    }

    pub fn cambiar_posicion(&mut self, lat: f64, long: f64) {
        self.posicion.set(lat, long);
    }
}
