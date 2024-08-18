#[derive(Debug)]
pub enum SistemaMonitoreoError {
    IdInvalido,
    InternalError,
    AppConfigError,
}

impl std::fmt::Display for SistemaMonitoreoError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            SistemaMonitoreoError::IdInvalido => write!(f, "El id es invalido"),
            SistemaMonitoreoError::InternalError => {
                write!(f, "Error tecnico al realizar una operacion")
            }
            SistemaMonitoreoError::AppConfigError => {
                write!(f, "Error en la configuracion de la aplicacion")
            }
        }
    }
}
