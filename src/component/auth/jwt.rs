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
pub struct UserUuid(pub uuid::Uuid);

impl FromRequest for UserUuid {
  type Error = actix_web::Error;

  type Future = std::future::Ready<Result<Self, Self::Error>>;

  fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
    let auth = get_auth_from_request(req);
    match auth {
      Ok(auth) => {
        let sub = auth.claims.sub.ok_or(actix_web::error::ErrorUnauthorized(
          "Invalid Authorization header, missing sub(uuid)",
        ));
        match sub {
          Ok(sub) => std::future::ready(Ok(UserUuid(uuid::Uuid::parse_str(&sub).unwrap()))),
          Err(e) => std::future::ready(Err(e)),
        }
      },
      Err(e) => std::future::ready(Err(e)),
    }
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Authorization {
  pub token: String,
  pub claims: GoTrueJWTClaims,
}

impl Authorization {
  pub fn uuid(&self) -> Option<String> {
    self.claims.sub.clone()
  }
}

impl FromRequest for Authorization {
  type Error = actix_web::Error;

  type Future = std::future::Ready<Result<Self, Self::Error>>;

  fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
    let auth = get_auth_from_request(req);
    match auth {
      Ok(auth) => std::future::ready(Ok(auth)),
      Err(e) => std::future::ready(Err(e)),
    }
  }
}

fn get_auth_from_request(req: &HttpRequest) -> Result<Authorization, actix_web::Error> {
  let state = req.app_data::<Data<State>>().unwrap();
  let bearer = req.headers().get("Authorization");
  match bearer {
    None => Err(actix_web::error::ErrorUnauthorized(
      "No Authorization header",
    )),
    Some(bearer) => {
      let bearer = bearer.to_str();
      match bearer {
        Err(e) => Err(actix_web::error::ErrorUnauthorized(e)),
        Ok(bearer) => {
          let pair_opt = bearer.split_once("Bearer "); // Authorization: Bearer <token>
          match pair_opt {
            None => Err(actix_web::error::ErrorUnauthorized(
              "Invalid Authorization header, missing Bearer",
            )),
            Some(pair) => {
              match GoTrueJWTClaims::verify(
                pair.1,
                state.config.gotrue.jwt_secret.expose_secret().as_bytes(),
              ) {
                Err(e) => Err(actix_web::error::ErrorUnauthorized(e)),
                Ok(t) => Ok(Authorization {
                  token: pair.1.to_string(),
                  claims: t,
                }),
              }
            },
          }
        },
      }
    },
  }
}

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

impl GoTrueJWTClaims {
  pub fn verify(token: &str, secret: &[u8]) -> Result<Self, jsonwebtoken::errors::Error> {
    Ok(decode(token, &DecodingKey::from_secret(secret), &VALIDATION)?.claims)
  }
}
