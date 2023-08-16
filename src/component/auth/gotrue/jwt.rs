use actix_http::Payload;
use actix_web::{FromRequest, HttpRequest};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

lazy_static::lazy_static! {
  pub static ref VALIDATION: Validation = Validation::new(Algorithm::HS256);
}

impl GoTrueJWTClaims {
  pub fn decode_from_str(token: &str, secret: &[u8]) -> Result<Self, jsonwebtoken::errors::Error> {
    Ok(decode(token, &DecodingKey::from_secret(secret), &VALIDATION)?.claims)
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

impl FromRequest for GoTrueJWTClaims {
  type Error = actix_web::Error;

  type Future = std::future::Ready<Result<Self, Self::Error>>;

  fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
    let bearer = req.headers().get("Authorization");
    match bearer {
      Some(bearer) => {
        let bearer = bearer.to_str().unwrap();
        // let token = bearer.replace("Bearer ", "");
        let token = bearer.split_once("Bearer ").unwrap().1;
        let claim = Self::decode_from_str(token, "todo".as_bytes());
        std::future::ready(Ok(claim.unwrap()))
      },
      None => std::future::ready(Err(actix_web::error::ErrorUnauthorized(
        "No Authorization header",
      ))),
    }
  }
}
