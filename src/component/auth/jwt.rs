use actix_http::Payload;
use actix_web::{web::Data, FromRequest, HttpRequest};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use realtime::entities::RealtimeUser;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::ops::Deref;

use crate::state::State;

lazy_static::lazy_static! {
  pub static ref VALIDATION: Validation = Validation::new(Algorithm::HS256);
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUuid {
  uuid: uuid::Uuid,
  uuid_str: String,
}

impl UserUuid {
  fn new(uuid: uuid::Uuid) -> Self {
    Self {
      uuid,
      uuid_str: uuid.to_string(),
    }
  }

  pub fn from_auth(auth: Authorization) -> Result<Self, actix_web::Error> {
    let uuid = auth
      .claims
      .sub
      .ok_or(actix_web::error::ErrorUnauthorized(
        "Invalid Authorization header, missing sub(uuid)",
      ))
      .map(|sub| {
        uuid::Uuid::parse_str(&sub).map_err(|e| {
          actix_web::error::ErrorUnauthorized(format!(
            "Invalid Authorization header, invalid sub(uuid): {}",
            e
          ))
        })
      })?;
    Ok(Self::new(uuid?))
  }
}

impl Deref for UserUuid {
  type Target = uuid::Uuid;

  fn deref(&self) -> &Self::Target {
    &self.uuid
  }
}

impl FromRequest for UserUuid {
  type Error = actix_web::Error;

  type Future = std::future::Ready<Result<Self, Self::Error>>;

  fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
    let auth = get_auth_from_request(req);
    match auth {
      Ok(auth) => match UserUuid::from_auth(auth) {
        Ok(uuid) => std::future::ready(Ok(uuid)),
        Err(e) => std::future::ready(Err(e)),
      },
      Err(e) => std::future::ready(Err(e)),
    }
  }
}

impl Display for UserUuid {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_str(&self.uuid_str)
  }
}

impl RealtimeUser for UserUuid {
  fn id(&self) -> &str {
    &self.uuid_str
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
  let bearer = req
    .headers()
    .get("Authorization")
    .ok_or(actix_web::error::ErrorUnauthorized(
      "No Authorization header",
    ))?;

  let bearer_str = bearer
    .to_str()
    .map_err(actix_web::error::ErrorUnauthorized)?;

  let (_, token) = bearer_str
    .split_once("Bearer ")
    .ok_or(actix_web::error::ErrorUnauthorized(
      "Invalid Authorization header, missing Bearer",
    ))?;

  authorization_from_token(token, state)
}

pub fn authorization_from_token(
  token: &str,
  state: &Data<State>,
) -> Result<Authorization, actix_web::Error> {
  let claims = gotrue_jwt_claims_from_token(token, state)?;
  Ok(Authorization {
    token: token.to_string(),
    claims,
  })
}

fn gotrue_jwt_claims_from_token(
  token: &str,
  state: &Data<State>,
) -> Result<GoTrueJWTClaims, actix_web::Error> {
  GoTrueJWTClaims::verify(
    token,
    state.config.gotrue.jwt_secret.expose_secret().as_bytes(),
  )
  .map_err(actix_web::error::ErrorUnauthorized)
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
