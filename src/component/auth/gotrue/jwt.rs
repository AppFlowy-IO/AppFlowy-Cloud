use actix_http::Payload;
use actix_web::{web::Data, FromRequest, HttpRequest};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};

use crate::state::State;

lazy_static::lazy_static! {
  pub static ref VALIDATION: Validation = Validation::new(Algorithm::HS256);
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Authorization {
  pub token: String,
  pub claims: GoTrueJWTClaims,
}

impl FromRequest for Authorization {
  type Error = actix_web::Error;

  type Future = std::future::Ready<Result<Self, Self::Error>>;

  fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
    let state = req.app_data::<Data<State>>().unwrap();
    let bearer = req.headers().get("Authorization");
    match bearer {
      None => std::future::ready(Err(actix_web::error::ErrorUnauthorized(
        "No Authorization header",
      ))),
      Some(bearer) => {
        let bearer = bearer.to_str();
        match bearer {
          Err(e) => std::future::ready(Err(actix_web::error::ErrorUnauthorized(e))),
          Ok(bearer) => {
            let pair_opt = bearer.split_once("Bearer "); // Authorization: Bearer <token>
            match pair_opt {
              None => std::future::ready(Err(actix_web::error::ErrorUnauthorized(
                "Invalid Authorization header, missing Bearer",
              ))),
              Some(pair) => {
                match GoTrueJWTClaims::verify(
                  pair.1,
                  state.config.gotrue.jwt_secret.expose_secret().as_bytes(),
                ) {
                  Err(e) => std::future::ready(Err(actix_web::error::ErrorUnauthorized(e))),
                  Ok(t) => std::future::ready(Ok(Authorization {
                    token: pair.1.to_string(),
                    claims: t,
                  })),
                }
              },
            }
          },
        }
      },
    }
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
