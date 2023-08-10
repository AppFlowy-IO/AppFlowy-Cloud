use anyhow::Error;
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

lazy_static::lazy_static! {
  pub static ref VALIDATION: Validation = Validation::new(Algorithm::HS256);
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Token {
  Anon(Anon),
  Authenticated(Authenticated),
}

impl Token {
  pub fn decode_from_str(&self, token: &str, secret: &[u8]) -> Result<Token, Error> {
    let token_data = decode::<Token>(&token, &DecodingKey::from_secret(secret), &VALIDATION)?;
    Ok(token_data.claims)
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Authenticated {
  aud: String,
  exp: u64,
  iat: u64,
  iss: String,
  sub: String,
  email: String,
  phone: String,
  app_metadata: AppMetadata,
  user_metadata: std::collections::HashMap<String, String>, // or another struct if you know the fields
  role: String,
  aal: String,
  amr: Vec<Amr>,
  session_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Anon {
  iss: String,
  #[serde(rename = "ref")]
  reference: String,
  role: String,
  iat: u64,
  exp: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct AppMetadata {
  provider: String,
  providers: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Amr {
  method: String,
  timestamp: u64,
}
