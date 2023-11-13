use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UserMessage {
  ProfileChange(AFUserChange),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AFUserChange {
  pub name: Option<String>,
  pub email: Option<String>,
  pub metadata: Option<String>,
}
