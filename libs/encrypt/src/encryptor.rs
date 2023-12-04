use anyhow::Error;
use bytes::Bytes;

pub trait DataEncryptor {
  fn encrypt(&self, data: Bytes) -> Result<Bytes, Error>;
  fn decrypt(&self, data: Bytes) -> Result<Bytes, Error>;
}

pub struct NoopEncryptor;

impl DataEncryptor for NoopEncryptor {
  fn encrypt(&self, data: Bytes) -> Result<Bytes, Error> {
    Ok(data)
  }

  fn decrypt(&self, data: Bytes) -> Result<Bytes, Error> {
    Ok(data)
  }
}
