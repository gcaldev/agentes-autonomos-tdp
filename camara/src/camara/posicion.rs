/// Posicion: Representa una Posicionenada en el plano 2d
#[derive(Debug, Clone, PartialEq)]
pub struct Posicion {
    pub lat: f64,
    pub long: f64,
}

impl Posicion {
    const fn _new(lat: f64, long: f64) -> Self {
        Posicion { lat, long }
    }

    pub fn en_rango(&self, lat: f64, long: f64, area: f64) -> bool {
        (self.lat - lat).abs() <= area && (self.long - long).abs() <= area
    }
}
