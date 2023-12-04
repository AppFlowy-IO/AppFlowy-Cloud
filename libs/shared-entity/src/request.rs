use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub enum RequestData {
  Json(Vec<u8>),
  Encrypted(Vec<u8>),
}
