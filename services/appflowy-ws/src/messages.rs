use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {}

impl Message {
  pub fn from_bytes(&self) -> Result<Self, Error> {
    //TODO
  }

  pub fn into_bytes(&self) -> Result<Self, Error> {
    //TODO
  }
}
