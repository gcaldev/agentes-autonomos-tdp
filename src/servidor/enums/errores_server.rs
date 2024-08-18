use std::fmt;
#[derive(Debug)]
/// Estructura para todos los errores que puede tener el servidor.
pub enum ErroresServer {
    UnknownProtocolOperation,
    AttemptedToConnectToRoutePort,
    AuthorizationViolation,
    AuthorizationTimeout,
    InvalidClientProtocol,
    MaximumControlLineExceeded,
    ParserError,
    SecureConnectionTLSRequired,
    StaleConnection,
    MaximumConnectionsExceeded,
    SlowConsumer,
    MaximumPayloadViolation,
    InvalidSubject(String),
    SubscriptionPermissionsViolation(String),
    PublishPermissionsViolation(String),
    InternalError(String),
    StreamsAccessError,
    MalformedUnsubError,
    InvalidSID,
    ClientDisconnected(String),
    InvalidJson,
    StreamNotFound,
}

impl fmt::Display for ErroresServer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ErroresServer::UnknownProtocolOperation => write!(f, "Unknown protocol operation"),
            ErroresServer::AttemptedToConnectToRoutePort => {
                write!(f, "Attempted to connect to route port")
            }
            ErroresServer::AuthorizationViolation => write!(f, "Authorization Violation"),
            ErroresServer::AuthorizationTimeout => write!(f, "Authorization timeout"),
            ErroresServer::InvalidClientProtocol => write!(f, "Invalid client protocol"),
            ErroresServer::MaximumControlLineExceeded => write!(f, "Maximum control line exceeded"),
            ErroresServer::ParserError => write!(f, "Parser error"),
            ErroresServer::SecureConnectionTLSRequired => {
                write!(f, "Secure connection TLS required")
            }
            ErroresServer::StaleConnection => write!(f, "Stale connection"),
            ErroresServer::MaximumConnectionsExceeded => write!(f, "Maximum connections exceeded"),
            ErroresServer::SlowConsumer => write!(f, "Slow consumer"),
            ErroresServer::MaximumPayloadViolation => write!(f, "Maximum payload violation"),
            ErroresServer::InvalidSubject(msg) => write!(f, "Invalid subject, {}", msg),
            ErroresServer::SubscriptionPermissionsViolation(subject) => {
                write!(f, "Permissions Violation for Subscription to {}", subject)
            }
            ErroresServer::PublishPermissionsViolation(subject) => {
                write!(f, "Permissions Violation for Publish to {}", subject)
            }
            ErroresServer::InternalError(msg) => write!(f, "Internal error: {} ", msg),
            ErroresServer::MalformedUnsubError => {
                write!(f, "Formato incorrecto de comando de desuscripcion")
            }
            ErroresServer::InvalidSID => write!(f, "SID invalido"),
            ErroresServer::ClientDisconnected(msg) => {
                write!(f, "Cliente con id {} desconectado", msg)
            }
            ErroresServer::InvalidJson => write!(f, "Invlaid Json"),
            ErroresServer::StreamNotFound => write!(f, "Stream not found"),
            ErroresServer::StreamsAccessError => write!(f, "Error al acceder a los streams"),
        }
    }
}

impl From<ErroresServer> for std::io::Error {
    fn from(err: ErroresServer) -> Self {
        match err {
            ErroresServer::UnknownProtocolOperation => {
                std::io::Error::new(std::io::ErrorKind::Other, "Unknown protocol operation")
            }
            ErroresServer::AttemptedToConnectToRoutePort => std::io::Error::new(
                std::io::ErrorKind::Other,
                "Attempted to connect to route port",
            ),
            ErroresServer::AuthorizationViolation => {
                std::io::Error::new(std::io::ErrorKind::Other, "Authorization violation")
            }
            ErroresServer::AuthorizationTimeout => {
                std::io::Error::new(std::io::ErrorKind::Other, "Authorization timeout")
            }
            ErroresServer::InvalidClientProtocol => {
                std::io::Error::new(std::io::ErrorKind::Other, "Invalid client protocol")
            }
            ErroresServer::MaximumControlLineExceeded => {
                std::io::Error::new(std::io::ErrorKind::Other, "Maximum control line exceeded")
            }
            ErroresServer::ParserError => {
                std::io::Error::new(std::io::ErrorKind::Other, "Parser error")
            }
            ErroresServer::SecureConnectionTLSRequired => {
                std::io::Error::new(std::io::ErrorKind::Other, "Secure connection TLS required")
            }
            ErroresServer::StaleConnection => {
                std::io::Error::new(std::io::ErrorKind::Other, "Stale connection")
            }
            ErroresServer::MaximumConnectionsExceeded => {
                std::io::Error::new(std::io::ErrorKind::Other, "Maximum connections exceeded")
            }
            ErroresServer::SlowConsumer => {
                std::io::Error::new(std::io::ErrorKind::Other, "Slow consumer")
            }
            ErroresServer::MaximumPayloadViolation => {
                std::io::Error::new(std::io::ErrorKind::Other, "Maximum payload violation")
            }
            ErroresServer::InvalidSubject(msg) => std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Invalid subject, {}", msg),
            ),
            ErroresServer::SubscriptionPermissionsViolation(reason) => std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Subscription permissions violation: {}", reason),
            ),
            ErroresServer::PublishPermissionsViolation(reason) => std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Publish permissions violation: {}", reason),
            ),
            ErroresServer::InternalError(msg) => std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Internal error: {}", msg),
            ),
            ErroresServer::MalformedUnsubError => std::io::Error::new(
                std::io::ErrorKind::Other,
                "Formato incorrecto de comando de desuscripcion",
            ),
            ErroresServer::InvalidSID => {
                std::io::Error::new(std::io::ErrorKind::Other, "SID invalido")
            }
            ErroresServer::ClientDisconnected(msg) => std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Cliente de id {} desconectado", msg),
            ),
            ErroresServer::InvalidJson => {
                std::io::Error::new(std::io::ErrorKind::Other, "Invalid Json")
            }
            ErroresServer::StreamNotFound => {
                std::io::Error::new(std::io::ErrorKind::Other, "Stream not found")
            }
            ErroresServer::StreamsAccessError => {
                std::io::Error::new(std::io::ErrorKind::Other, "Error al acceder a los streams")
            }
        }
    }
}
