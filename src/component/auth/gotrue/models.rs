use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize)]
pub struct SignUpResponse {
  id: Uuid,
  aud: String,
  role: String,
  email: String,
  phone: String,
  confirmation_sent_at: String,
  app_metadata: AppMetadata,
  user_metadata: HashMap<String, String>,
  identities: Vec<Identity>,
  created_at: String,
  updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppMetadata {
  provider: String,
  providers: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Identity {
  id: Uuid,
  user_id: Uuid,
  identity_data: IdentityData,
  provider: String,
  last_sign_in_at: String,
  created_at: String,
  updated_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct IdentityData {
  email: String,
  sub: Uuid,
}
