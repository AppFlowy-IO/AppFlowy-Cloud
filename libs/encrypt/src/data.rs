use crate::encryptor::DataEncryptor;
use anyhow::Error;
use bytes::Bytes;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptionData(Bytes);

impl EncryptionData {
  pub fn from_data<T: Serialize, E: DataEncryptor>(data: &T, encryptor: &E) -> Result<Self, Error> {
    let data = bincode::serialize(data)?;
    let encryption_data = encryptor.encrypt(Bytes::from(data))?;
    Ok(EncryptionData(encryption_data))
  }

  pub fn decrypt<T: DeserializeOwned, E: DataEncryptor>(self, encryptor: &E) -> Result<T, Error> {
    let decryption_data = encryptor.decrypt(self.0)?;
    bincode::deserialize(&decryption_data).map_err(|err| err.into())
  }
}

impl TryFrom<Bytes> for EncryptionData {
  type Error = Error;

  fn try_from(value: Bytes) -> Result<Self, Self::Error> {
    bincode::deserialize(&value).map_err(|err| err.into())
  }
}

impl TryFrom<EncryptionData> for Vec<u8> {
  type Error = Error;

  fn try_from(value: EncryptionData) -> Result<Self, Self::Error> {
    bincode::serialize(&value).map_err(|err| err.into())
  }
}
