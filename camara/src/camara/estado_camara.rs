use std::fmt;

#[derive(Debug, Clone, Eq, PartialEq)]
/// Representa los posibles estados de una camara
pub enum EstadoCamara {
    AhorroDeEnergia,
    Grabando,
}

// Implementación del trait Display para EstadoCamara
impl fmt::Display for EstadoCamara {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            EstadoCamara::AhorroDeEnergia => write!(f, "Ahorro de Energía"),
            EstadoCamara::Grabando => write!(f, "Grabando"),
        }
    }
}
