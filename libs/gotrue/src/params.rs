use std::collections::btree_map::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize)]
pub struct AdminUserParams {
  pub aud: String,
  pub role: String,
  pub email: String,
  pub phone: String,
  pub password: Option<String>,
  pub email_confirm: bool,
  pub phone_confirm: bool,
  pub user_metadata: BTreeMap<String, serde_json::Value>,
  pub app_metadata: BTreeMap<String, serde_json::Value>,
  pub ban_duration: String,
}
