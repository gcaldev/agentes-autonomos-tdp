extern crate crypto as rust_crypto;
use rust_crypto::{
    aes, blockmodes,
    buffer::{self, BufferResult, ReadBuffer, WriteBuffer},
    symmetriccipher,
};

use super::errors::CryptoError;

const BCRYPT_HASH: &str = "$2b$12$9XmsdBtXYrDfZjKpsIvfp.WTIAnU/05JQcogfvb5OhiLGxKQXLWE2";

fn _encrypt(
    data: &[u8],
    key: &[u8],
    iv: &[u8],
) -> Result<Vec<u8>, symmetriccipher::SymmetricCipherError> {
    let mut encryptor =
        aes::cbc_encryptor(aes::KeySize::KeySize256, key, iv, blockmodes::PkcsPadding);

    let mut encrypted_data = Vec::<u8>::new();
    let mut read_buffer = buffer::RefReadBuffer::new(data);
    let mut buffer = [0; 4096];
    let mut write_buffer = buffer::RefWriteBuffer::new(&mut buffer);

    loop {
        let result = encryptor.encrypt(&mut read_buffer, &mut write_buffer, true)?;

        encrypted_data.extend(
            write_buffer
                .take_read_buffer()
                .take_remaining()
                .iter()
                .copied(),
        );

        match result {
            BufferResult::BufferUnderflow => break,
            BufferResult::BufferOverflow => {}
        }
    }

    Ok(encrypted_data)
}

fn _decrypt(
    encrypted_data: &[u8],
    key: &[u8],
    iv: &[u8],
) -> Result<Vec<u8>, symmetriccipher::SymmetricCipherError> {
    let mut decryptor =
        aes::cbc_decryptor(aes::KeySize::KeySize256, key, iv, blockmodes::PkcsPadding);

    let mut decrypted_data = Vec::<u8>::new();
    let mut read_buffer = buffer::RefReadBuffer::new(encrypted_data);
    let mut buffer = [0; 4096];
    let mut write_buffer = buffer::RefWriteBuffer::new(&mut buffer);

    loop {
        let result = decryptor.decrypt(&mut read_buffer, &mut write_buffer, true)?;
        decrypted_data.extend(
            write_buffer
                .take_read_buffer()
                .take_remaining()
                .iter()
                .copied(),
        );
        match result {
            BufferResult::BufferUnderflow => break,
            BufferResult::BufferOverflow => {}
        }
    }

    Ok(decrypted_data)
}

/// Obtiene la clave y el vector de inicialización para encriptar y desencriptar.
fn get_key_iv() -> (Vec<u8>, Vec<u8>) {
    let key = BCRYPT_HASH.as_bytes()[..32].to_vec();
    let iv = BCRYPT_HASH.as_bytes()[32..48].to_vec();
    (key, iv)
}

/// Encripta datos usando el algoritmo AES-256 CBC con padding PKCS.
pub fn encrypt(data: &[u8]) -> Result<Vec<u8>, symmetriccipher::SymmetricCipherError> {
    let (key, iv) = get_key_iv();
    _encrypt(data, &key, &iv)
}

/// Desencripta datos encriptados con el algoritmo AES-256 CBC con padding PKCS.
pub fn decrypt(encrypted_data: &[u8]) -> Result<Vec<u8>, symmetriccipher::SymmetricCipherError> {
    let (key, iv) = get_key_iv();
    _decrypt(encrypted_data, &key, &iv)
}

/// Desencripta datos encriptados con el algoritmo AES-256 CBC con padding PKCS separados por CRLF.
pub fn decrypt_with_crlf(msg: &[u8]) -> Result<String, CryptoError> {
    let mut result = String::new();
    let mut buffer = Vec::new();
    let mut last_byte = 0;
    let mut i = 0;
    while i < msg.len() {
        let byte = msg[i];
        i += 1;
        if last_byte == b'\r' && byte == b'\n' {
            buffer.pop();
            let decrypted_data = decrypt(&buffer).map_err(|_| CryptoError::DecryptionError)?;
            result.push_str(
                &String::from_utf8(decrypted_data).map_err(|_| CryptoError::DecryptionError)?,
            );
            result.push_str("\r\n");
            buffer.clear();
            continue;
        }
        buffer.push(byte);
        last_byte = byte;
    }
    if !buffer.is_empty() {
        let decrypted_data = decrypt(&buffer).map_err(|_| CryptoError::DecryptionError)?;
        result.push_str(
            &String::from_utf8(decrypted_data).map_err(|_| CryptoError::DecryptionError)?,
        );
    }
    Ok(result)
}

/// Encripta datos usando el algoritmo AES-256 CBC con padding PKCS separados por CRLF.
pub fn encrypted_with_crlf(msg: &str) -> Result<Vec<u8>, CryptoError> {
    let mut buf: Vec<u8> = Vec::new();
    let msg_split_len = msg.split("\r\n").count();
    for (index, line) in msg.split("\r\n").enumerate() {
        let encrypted_msg = encrypt(line.as_bytes()).map_err(|_| CryptoError::EncryptionError)?;
        if index + 1 < msg_split_len {
            let with_crlf: Vec<u8> = encrypted_msg
                .iter()
                .chain(&[b'\r', b'\n'])
                .cloned()
                .collect();
            buf.extend(with_crlf);
        } else {
            buf.extend(encrypted_msg);
        }
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypt_decrypt() {
        let data = b"Hello, world!";

        let encrypted_data = encrypt(data).unwrap();
        let decrypted_data = decrypt(&encrypted_data).unwrap();

        assert_eq!(data, &decrypted_data[..]);
    }

    #[test]
    fn test_encrypt_decrypt_with_crlf() {
        let data = "Hello, world!\r\nThis is a test\r\n";

        let encrypted_data = encrypted_with_crlf(data).unwrap();
        let decrypted_data = decrypt_with_crlf(&encrypted_data).unwrap();

        assert_eq!(data, &decrypted_data);
    }

    #[test]
    fn test_encrypt_decrypt_with_crlf_without_end_crlf() {
        let data = "Hello, world!\r\nThis is a test";

        let encrypted_data = encrypted_with_crlf(data).unwrap();
        let decrypted_data = decrypt_with_crlf(&encrypted_data).unwrap();

        assert_eq!(data, &decrypted_data);
    }

    #[test]
    fn test_malformed_decrypt() {
        let data = b"Hello, world!";
        let mut encrypted_data = encrypt(data).unwrap();
        encrypted_data[0] = 0;

        let result = decrypt(&encrypted_data);

        assert!(result.is_err());
    }

    #[test]
    fn test_malformed_decrypt_with_crlf() {
        let data = "Hello, world!\r\nThis is a test\r\n";
        let mut encrypted_data = encrypted_with_crlf(data).unwrap();
        encrypted_data[0] = 0;

        let result = decrypt_with_crlf(&encrypted_data);

        assert!(matches!(result, Err(CryptoError::DecryptionError)));
    }
}
