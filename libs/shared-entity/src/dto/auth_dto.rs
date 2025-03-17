// Data Transfer Objects (DTO)

use gotrue_entity::dto::GotrueTokenResponse;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::workspace_dto::ViewIcon;

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

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct DeleteUserQuery {
  pub provider_access_token: Option<String>,
  pub provider_refresh_token: Option<String>,
}

#[derive(serde::Deserialize, serde::Serialize, Default)]
pub struct CreateNewPageParams {
  pub parent_view_id: String,
  pub view_layout: u32,
  pub name: Option<String>,
  pub page_data: Option<serde_json::Value>,
}

#[derive(serde::Deserialize, serde::Serialize, Default)]
pub struct UpdatePageParams {
  pub name: Option<String>,
  pub icon: Option<ViewIcon>,
  pub is_locked: Option<bool>,
  pub extra: Option<serde_json::Value>,
}

impl UpdatePageParams {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn with_name<T: ToString>(mut self, name: T) -> Self {
    self.name = Some(name.to_string());
    self
  }

  pub fn with_icon<T: Into<ViewIcon>>(mut self, icon: T) -> Self {
    self.icon = Some(icon.into());
    self
  }
  pub fn with_is_locked<T: Into<bool>>(mut self, is_locked: T) -> Self {
    self.is_locked = Some(is_locked.into());
    self
  }
  pub fn with_extra<T: Into<serde_json::Value>>(mut self, extra: T) -> Self {
    self.extra = Some(extra.into());
    self
  }
}
