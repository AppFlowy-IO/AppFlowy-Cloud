use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct GoTrueJWTClaims {
  // JWT standard claims
  pub aud: Option<String>,
  pub exp: Option<i64>,
  pub jti: Option<String>,
  pub iat: Option<i64>,
  pub iss: Option<String>,
  pub nbf: Option<i64>,
  pub sub: Option<String>,

  pub email: String,
  pub phone: String,
  pub app_metadata: serde_json::Value,
  pub user_metadata: serde_json::Value,
  pub role: String,
  pub aal: Option<String>,
  pub amr: Option<Vec<Amr>>,
  pub session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Amr {
  pub method: String,
  pub timestamp: u64,
  pub provider: Option<String>,
}

lazy_static::lazy_static! {
  pub static ref VALIDATION: Validation = Validation::new(Algorithm::HS256);
}

impl GoTrueJWTClaims {
  pub fn verify(token: &str, secret: &[u8]) -> Result<Self, jsonwebtoken::errors::Error> {
    Ok(decode(token, &DecodingKey::from_secret(secret), &VALIDATION)?.claims)
  }
}
