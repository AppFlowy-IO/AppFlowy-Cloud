use std::collections::btree_map::BTreeMap;

use gotrue_entity::dto::{Factor, Identity};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct AdminDeleteUserParams {
  pub should_soft_delete: bool,
}

#[derive(Default, Serialize)]
pub struct InviteUserParams {
  pub email: String,
  pub data: serde_json::Value,
}

#[derive(Debug, Default, Deserialize, Serialize)]
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

#[derive(Deserialize, Serialize)]
pub struct GenerateLinkParams {
  #[serde(rename = "type")]
  pub type_: GenerateLinkType,

  pub email: String,
  pub new_email: String,
  pub password: String,
  pub data: BTreeMap<String, serde_json::Value>,
  pub redirect_to: String,
}

#[derive(Default, Deserialize, Serialize)]
pub struct MagicLinkParams {
  pub email: String,
  pub data: BTreeMap<String, serde_json::Value>,
  pub code_challenge_method: String,
  pub code_challenge: String,
}

impl Default for GenerateLinkParams {
  fn default() -> Self {
    GenerateLinkParams {
      type_: GenerateLinkType::MagicLink,
      email: String::default(),
      new_email: String::default(),
      password: String::default(),
      data: BTreeMap::new(),
      redirect_to: "appflowy-flutter://".to_string(),
    }
  }
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum GenerateLinkType {
  MagicLink,
  Recovery,
  Invite,
  Signup,
  EmailChange,
  PhoneChange,
  Reauthenticate,
  Sms,
  Email,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GenerateLinkResponse {
  // putting User here as Rust does not support struct field extension
  // use gotrue_entity::User
  pub id: String,
  pub aud: String,
  pub role: String,
  pub email: String,
  pub email_confirmed_at: Option<String>,
  pub invited_at: Option<String>,
  pub phone: String,
  pub phone_confirmed_at: Option<String>,
  pub confirmation_sent_at: Option<String>,
  pub confirmed_at: Option<String>,
  pub recovery_sent_at: Option<String>,
  pub new_email: Option<String>,
  pub email_change_sent_at: Option<String>,
  pub new_phone: Option<String>,
  pub phone_change_sent_at: Option<String>,
  pub reauthentication_sent_at: Option<String>,
  pub last_sign_in_at: Option<String>,
  pub app_metadata: serde_json::Value,
  pub user_metadata: serde_json::Value,
  pub factors: Option<Vec<Factor>>,
  pub identities: Vec<Identity>,
  pub created_at: String,
  pub updated_at: String,
  pub banned_until: Option<String>,
  pub deleted_at: Option<String>,
  //
  pub action_link: String,
  pub email_otp: String,
  pub hashed_token: String,
  pub verification_type: String,
  pub redirect_to: String,
}

#[derive(Debug, Serialize, Default)]
pub struct CreateSSOProviderParams {
  #[serde(rename = "type")]
  pub type_: String,
  pub metadata_url: String,
  pub metadata_xml: String,
  pub domains: Vec<String>,
  pub attribute_mapping: serde_json::Value,
}
