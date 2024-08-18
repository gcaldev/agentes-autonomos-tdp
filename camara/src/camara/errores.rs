#[derive(Debug)]
pub enum ErroresCentral {
    ErrorAlCrearCamara,
    NoSePudoBorrarLaCamara,
    ErrorAlProcesarIncidente,
    InternalError(String),
}

impl std::fmt::Display for ErroresCentral {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            ErroresCentral::ErrorAlCrearCamara => write!(f, "Hubo un error al crear la camara"),
            ErroresCentral::NoSePudoBorrarLaCamara => write!(
                f,
                "El id pasado por parametro no le pertenece a ninguna camara"
            ),
            ErroresCentral::InternalError(msg) => write!(f, "Error interno: {}", msg),
            ErroresCentral::ErrorAlProcesarIncidente => write!(f, "Error al procesar incidente"),
        }
    }
}
impl From<ErroresCentral> for std::io::Error {
    fn from(err: ErroresCentral) -> Self {
        match err {
            ErroresCentral::ErrorAlCrearCamara => std::io::Error::new(
                std::io::ErrorKind::Other,
                "Hubo un error al crear la camara",
            ),
            ErroresCentral::NoSePudoBorrarLaCamara => std::io::Error::new(
                std::io::ErrorKind::Other,
                "El id pasado por parametro no le pertenece a ninguna camara",
            ),
            ErroresCentral::InternalError(msg) => {
                std::io::Error::new(std::io::ErrorKind::Other, format!("Error interno: {}", msg))
            }
            ErroresCentral::ErrorAlProcesarIncidente => {
                std::io::Error::new(std::io::ErrorKind::Other, "Error al procesar incidente")
            }
        }
    }
}
