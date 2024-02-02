use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

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

impl Display for GoTrueJWTClaims {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.debug_struct("GoTrueJWTClaims")
      .field("exp", &self.exp)
      .field("email", &self.email)
      .finish()
  }
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
  pub fn decode(token: &str, secret: &[u8]) -> Result<Self, jsonwebtoken::errors::Error> {
    let token_data = decode::<Self>(token, &DecodingKey::from_secret(secret), &VALIDATION)?;
    Ok(token_data.claims)
  }

  pub fn verify_claim(claims: &GoTrueJWTClaims) -> Result<(), jsonwebtoken::errors::Error> {
    let ts_expiry = claims.exp.ok_or_else(|| {
      jsonwebtoken::errors::ErrorKind::MissingRequiredClaim("expect exp but not found".to_owned())
    })?;

    let ts_now = chrono::Utc::now().timestamp();
    if ts_now > ts_expiry {
      Err(jsonwebtoken::errors::ErrorKind::ExpiredSignature.into())
    } else {
      Ok(())
    }
  }
}
