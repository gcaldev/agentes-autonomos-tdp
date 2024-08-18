#[derive(Debug, Clone, PartialEq)]
/// Posicion: Representa una Posicionenada en el plano 2d
pub struct Posicion {
    pub lat: f64,
    pub long: f64,
}

impl Posicion {
    pub const fn new(lat: f64, long: f64) -> Self {
        Posicion { lat, long }
    }

    pub fn set(&mut self, lat: f64, long: f64) {
        self.lat = lat;
        self.long = long;
    }
}
