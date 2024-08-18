use core::fmt;
use std::fmt::Display;

#[derive(Debug, Clone)]
pub enum CryptoError {
    EncryptionError,
    DecryptionError,
}

impl Display for CryptoError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CryptoError::EncryptionError => write!(f, "Encryption error"),
            CryptoError::DecryptionError => write!(f, "Decryption error"),
        }
    }
}

impl From<CryptoError> for std::io::Error {
    fn from(err: CryptoError) -> Self {
        match err {
            CryptoError::EncryptionError => {
                std::io::Error::new(std::io::ErrorKind::Other, "Encryption error")
            }
            CryptoError::DecryptionError => {
                std::io::Error::new(std::io::ErrorKind::Other, "Decryption error")
            }
        }
    }
}
