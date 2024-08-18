use core::fmt;
use std::fmt::Display;

#[derive(Debug, Clone)]
pub enum NatsClientError {
    InvalidSubject,
    InvalidParameter(String),
    InternalError(String),
    SubscriptionNotFound,
    ServerError(String),
    MalformedMessage,
}

impl Display for NatsClientError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            NatsClientError::InvalidSubject => write!(f, "Invalid subject"),
            NatsClientError::InvalidParameter(param) => write!(f, "Invalid parameter: {}", param),
            NatsClientError::InternalError(msg) => write!(f, "Internal error: {}", msg),
            NatsClientError::SubscriptionNotFound => write!(f, "Subscription not found"),
            NatsClientError::ServerError(msg) => write!(f, "Server error: {}", msg),
            NatsClientError::MalformedMessage => write!(f, "Malformed message"),
        }
    }
}

impl From<NatsClientError> for std::io::Error {
    fn from(err: NatsClientError) -> Self {
        match err {
            NatsClientError::InvalidSubject => {
                std::io::Error::new(std::io::ErrorKind::Other, "Invalid subject")
            }
            NatsClientError::InvalidParameter(param) => std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Invalid parameter: {}", param),
            ),
            NatsClientError::InternalError(msg) => std::io::Error::new(
                std::io::ErrorKind::Other,
                format!("Internal error: {}", msg),
            ),
            NatsClientError::SubscriptionNotFound => {
                std::io::Error::new(std::io::ErrorKind::Other, "Subscription not found")
            }
            NatsClientError::ServerError(msg) => {
                std::io::Error::new(std::io::ErrorKind::Other, format!("Server error: {}", msg))
            }
            NatsClientError::MalformedMessage => {
                std::io::Error::new(std::io::ErrorKind::Other, "Malformed message")
            }
        }
    }
}
