use aes_gcm::aead::generic_array::GenericArray;
use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit};
use anyhow::{anyhow, Result};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hkdf::Hkdf;

use rand::Rng;
use sha2::Sha256;

/// The length of the derived encryption key in bytes.
const KEY_LENGTH: usize = 32;

/// The length of the nonce for AES-GCM encryption.
const NONCE_LENGTH: usize = 12;

/// Encrypts data using AES-256-GCM with a shared secret.
///
/// # Arguments
/// * `data` - Data to encrypt. Can be any type that implements `AsRef<[u8]>`.
/// * `shared_secret` - The shared secret used for key derivation. Can be any type that implements `AsRef<[u8]>`.
///
pub fn encrypt_data<T1: AsRef<[u8]>, T2: AsRef<[u8]>>(
  data: T1,
  shared_secret: T2,
) -> Result<Vec<u8>> {
  let key = derive_key(&shared_secret)?;
  let cipher = Aes256Gcm::new(GenericArray::from_slice(&key));
  let nonce: [u8; NONCE_LENGTH] = rand::thread_rng().gen();

  cipher
    .encrypt(GenericArray::from_slice(&nonce), data.as_ref())
    .map(|ciphertext| nonce.into_iter().chain(ciphertext).collect())
    .map_err(|e| anyhow!("Encryption error: {:?}", e))
}

/// Decrypts data encrypted by `encrypt_data`.
///
/// # Arguments
/// * `data` - Encrypted data to decrypt. Can be any type that implements `AsRef<[u8]>`.
/// * `shared_secret` - The shared secret used for key derivation. Can be any type that implements `AsRef<[u8]>`.
///
pub fn decrypt_data<T1: AsRef<[u8]>, T2: AsRef<[u8]>>(
  data: T1,
  shared_secret: T2,
) -> Result<Vec<u8>> {
  if data.as_ref().len() <= NONCE_LENGTH {
    return Err(anyhow::anyhow!("Ciphertext too short to include nonce."));
  }
  let key = derive_key(&shared_secret)?;
  let cipher = Aes256Gcm::new(GenericArray::from_slice(&key));
  let (nonce, cipher_data) = data.as_ref().split_at(NONCE_LENGTH);
  cipher
    .decrypt(GenericArray::from_slice(nonce), cipher_data)
    .map_err(|e| anyhow::anyhow!("Decryption error: {:?}", e))
}

pub fn encrypt_text<T1: AsRef<[u8]>, T2: AsRef<[u8]>>(
  data: T1,
  shared_secret: T2,
) -> Result<String> {
  let encrypted = encrypt_data(data.as_ref(), shared_secret)?;
  Ok(STANDARD.encode(encrypted))
}

pub fn decrypt_text<T1: AsRef<[u8]>, T2: AsRef<[u8]>>(
  data: T1,
  shared_secret: T2,
) -> Result<String> {
  let encrypted = STANDARD.decode(data)?;
  let decrypted = decrypt_data(encrypted, shared_secret)?;
  Ok(String::from_utf8(decrypted)?)
}

fn derive_key<T: AsRef<[u8]>>(shared_secret: &T) -> Result<[u8; KEY_LENGTH]> {
  let hkdf = Hkdf::<Sha256>::new(None, shared_secret.as_ref());
  let mut okm = [0u8; KEY_LENGTH];
  hkdf.expand(b"", &mut okm).expect("HKDF expansion failed");
  Ok(okm)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn encrypt_decrypt_test() {
    let secret =
      hex::decode("cc66c018bfe0a7af8ce0f98847d2ead96a9927df16111068bf98a79f40b39e00").unwrap();
    let data = b"hello world";
    let encrypted = encrypt_data(data, &secret).unwrap();
    let decrypted = decrypt_data(encrypted, &secret).unwrap();
    assert_eq!(data, decrypted.as_slice());

    let s = "123".to_string();
    let encrypted = encrypt_text(&s, &secret).unwrap();
    let decrypted_str = decrypt_text(encrypted, &secret).unwrap();
    assert_eq!(s, decrypted_str);
  }

  #[test]
  fn decrypt_with_invalid_secret_test() {
    let secret =
      hex::decode("cc66c018bfe0a7af8ce0f98847d2ead96a9927df16111068bf98a79f40b39e00").unwrap();
    let data = b"hello world";
    let encrypted = encrypt_data(data, secret.as_slice()).unwrap();
    let decrypted = decrypt_data(encrypted, "invalid secret".as_bytes());
    assert!(decrypted.is_err())
  }
}
