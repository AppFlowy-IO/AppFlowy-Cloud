use actix_http::Payload;
use actix_web::{web::Data, FromRequest, HttpRequest};

use gotrue_entity::gotrue_jwt::GoTrueJWTClaims;
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
use sqlx::types::{uuid, Uuid};
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use std::str::FromStr;
use tracing::instrument;

use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserUuid(uuid::Uuid);

impl UserUuid {
  pub fn from_auth(auth: Authorization) -> Result<Self, actix_web::Error> {
    Ok(Self(auth.uuid()?))
  }
}

impl Deref for UserUuid {
  type Target = uuid::Uuid;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

#[derive(Clone, Debug)]
pub struct UserToken(pub String);
impl UserToken {
  pub fn from_auth(auth: Authorization) -> Result<Self, actix_web::Error> {
    Ok(Self(auth.token))
  }
}

impl Display for UserToken {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_str(&self.0)
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

#[derive(Debug, Serialize, Deserialize)]
pub struct Authorization {
  pub token: String,
  pub claims: GoTrueJWTClaims,
}

impl Authorization {
  pub fn uuid(&self) -> Result<uuid::Uuid, actix_web::Error> {
    self
      .claims
      .sub
      .as_deref()
      .map(Uuid::from_str)
      .ok_or(actix_web::error::ErrorUnauthorized(
        "Invalid Authorization header, missing sub(uuid)",
      ))?
      .map_err(|e| {
        actix_web::error::ErrorUnauthorized(format!(
          "Invalid Authorization header, invalid sub(uuid): {}",
          e
        ))
      })
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

// impl RealtimeUser for Authorization {
//   fn id(&self) -> &str {
//     &self.uuid_str
//   }
// }

fn get_auth_from_request(req: &HttpRequest) -> Result<Authorization, actix_web::Error> {
  let state = req.app_data::<Data<AppState>>().unwrap();
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

#[instrument(level = "trace", skip_all, err)]
pub fn authorization_from_token(
  token: &str,
  state: &Data<AppState>,
) -> Result<Authorization, actix_web::Error> {
  let claims = gotrue_jwt_claims_from_token(token, state)?;
  Ok(Authorization {
    token: token.to_string(),
    claims,
  })
}

#[instrument(level = "trace", skip_all, err)]
fn gotrue_jwt_claims_from_token(
  token: &str,
  state: &Data<AppState>,
) -> Result<GoTrueJWTClaims, actix_web::Error> {
  let claims = GoTrueJWTClaims::decode(
    token,
    state.config.gotrue.jwt_secret.expose_secret().as_bytes(),
  )
  .map_err(|err| {
    actix_web::error::ErrorUnauthorized(format!("fail to decode token, error:{}", err))
  })?;
  Ok(claims)
}
