use actix_http::Payload;
use actix_web::{FromRequest, HttpRequest};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

lazy_static::lazy_static! {
  pub static ref VALIDATION: Validation = Validation::new(Algorithm::HS256);
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AuthBearerToken(String);
impl FromRequest for AuthBearerToken {
  type Error = actix_web::Error;

  type Future = std::future::Ready<Result<Self, Self::Error>>;

  fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
    let bearer = req.headers().get("Authorization");
    match bearer {
      Some(bearer) => {
        let bearer = bearer.to_str();
        match bearer {
          Ok(bearer) => {
            let token = bearer.split_once("Bearer ");
            match token {
              Some(token) => std::future::ready(Ok(Self(token.1.to_string()))),
              None => std::future::ready(Err(actix_web::error::ErrorUnauthorized(
                "Invalid Authorization header, missing Bearer",
              ))),
            }
          },
          Err(_) => std::future::ready(Err(actix_web::error::ErrorUnauthorized(
            "Invalid Authorization header",
          ))),
        }
      },
      None => std::future::ready(Err(actix_web::error::ErrorUnauthorized(
        "No Authorization header",
      ))),
    }
  }
}

impl AuthBearerToken {
  pub fn as_str(&self) -> &str {
    &self.0
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GoTrueJWTClaims {
  // JWT standard claims
  aud: Option<String>,
  exp: Option<i64>,
  jti: Option<String>,
  iat: Option<i64>,
  iss: Option<String>,
  nbf: Option<i64>,
  sub: Option<String>,

  email: String,
  phone: String,
  app_metadata: serde_json::Value,
  user_metadata: serde_json::Value,
  role: String,
  aal: Option<String>,
  amr: Option<Vec<Amr>>,
  session_id: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Amr {
  method: String,
  timestamp: u64,
  provider: Option<String>,
}

impl GoTrueJWTClaims {
  pub fn verify(token: &str, secret: &[u8]) -> Result<Self, jsonwebtoken::errors::Error> {
    Ok(decode(token, &DecodingKey::from_secret(secret), &VALIDATION)?.claims)
  }
}
