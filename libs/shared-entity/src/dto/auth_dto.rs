// Data Transfer Objects (DTO)

use gotrue_entity::dto::GotrueTokenResponse;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize, Serialize)]
pub struct SignInParams {
  pub email: String,
  pub password: String,
}

#[derive(Default, Deserialize, Serialize, Clone)]
pub struct UserMetaData(HashMap<String, serde_json::Value>);
impl UserMetaData {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn into_inner(self) -> HashMap<String, serde_json::Value> {
    self.0
  }

  pub fn insert<T: Into<serde_json::Value>>(&mut self, key: &str, value: T) {
    self.0.insert(key.to_string(), value.into());
  }
}

#[derive(serde::Deserialize, serde::Serialize, Default)]
pub struct UpdateUserParams {
  pub name: Option<String>,
  pub password: Option<String>,
  pub email: Option<String>,
  pub metadata: Option<UserMetaData>,
}

impl UpdateUserParams {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn with_password<T: ToString>(mut self, password: T) -> Self {
    self.password = Some(password.to_string());
    self
  }
  pub fn with_name<T: ToString>(mut self, name: T) -> Self {
    self.name = Some(name.to_string());
    self
  }
  pub fn with_email<T: ToString>(mut self, email: T) -> Self {
    self.email = Some(email.to_string());
    self
  }
  pub fn with_metadata<T: Into<UserMetaData>>(mut self, metadata: T) -> Self {
    self.metadata = Some(metadata.into());
    self
  }
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInPasswordResponse {
  pub access_token_resp: GotrueTokenResponse,
  pub is_new: bool,
}

#[derive(serde::Deserialize, serde::Serialize)]
pub struct SignInTokenResponse {
  pub is_new: bool,
}
