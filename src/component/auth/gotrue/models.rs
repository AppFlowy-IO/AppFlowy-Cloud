use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Identity {
  id: String,
  user_id: String,
  identity_data: Option<serde_json::Value>,
  provider: String,
  last_sign_in_at: String,
  created_at: String,
  updated_at: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct User {
  id: String,

  aud: String,
  role: String,
  email: String,

  email_confirmed_at: Option<String>,
  invited_at: Option<String>,

  phone: String,
  phone_confirmed_at: Option<String>,

  confirmation_sent_at: Option<String>,

  // For backward compatibility only. Use EmailConfirmedAt or PhoneConfirmedAt instead.
  confirmed_at: Option<String>,

  recovery_sent_at: Option<String>,

  new_email: Option<String>,
  email_change_sent_at: Option<String>,

  new_phone: Option<String>,
  phone_change_sent_at: Option<String>,

  reauthentication_sent_at: Option<String>,

  last_sign_in_at: Option<String>,

  app_metadata: serde_json::Value,
  user_metadata: serde_json::Value,

  factors: Option<Vec<Factor>>,
  identities: Vec<Identity>,

  created_at: String,
  updated_at: String,
  banned_until: Option<String>,
  deleted_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Factor {
  id: String,
  created_at: String,
  updated_at: String,
  status: String,
  friendly_name: Option<String>,
  factor_type: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct AccessTokenResponse {
  access_token: String,
  token_type: String,
  expires_in: i64,
  expires_at: i64,
  refresh_token: String,
  user: User,
  provider_access_token: Option<String>,
  provider_refresh_token: Option<String>,
}
