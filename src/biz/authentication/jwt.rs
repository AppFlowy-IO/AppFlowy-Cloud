use actix_http::Payload;
use actix_web::{web::Data, FromRequest, HttpRequest};

use gotrue_entity::gotrue_jwt::GoTrueJWTClaims;
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Serialize};
use sqlx::types::{uuid, Uuid};
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use std::str::FromStr;
use tracing::instrument;

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

// For cases where the handler itself will handle the request differently
// based on whether the user is authenticated or not
pub struct OptionalUserUuid(Option<UserUuid>);

impl FromRequest for OptionalUserUuid {
  type Error = actix_web::Error;

  type Future = std::future::Ready<Result<Self, Self::Error>>;

  fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
    let auth = get_auth_from_request(req);
    match auth {
      Ok(auth) => match UserUuid::from_auth(auth) {
        Ok(uuid) => std::future::ready(Ok(OptionalUserUuid(Some(uuid)))),
        Err(_) => std::future::ready(Ok(OptionalUserUuid(None))),
      },
      Err(_) => std::future::ready(Ok(OptionalUserUuid(None))),
    }
  }
}

impl OptionalUserUuid {
  pub fn as_uuid(&self) -> Option<uuid::Uuid> {
    self.0.as_deref().map(|uuid| uuid.to_owned())
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Authorization {
  pub token: String,
  pub claims: GoTrueJWTClaims,
}

impl Authorization {
  pub fn uuid(&self) -> Result<uuid::Uuid, actix_web::Error> {
    let sub = self.claims.sub.as_deref();
    match sub {
      None => Err(actix_web::error::ErrorUnauthorized(
        "Invalid Authorization header, missing sub(uuid)",
      )),
      Some(sub) => match Uuid::from_str(sub) {
        Ok(uuid) => Ok(uuid),
        Err(_) => Err(actix_web::error::ErrorUnauthorized(format!(
          "Invalid Authorization header, invalid sub: {}",
          sub
        ))),
      },
    }
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
  let jwt_secret_data =
    req
      .app_data::<Data<Secret<String>>>()
      .ok_or(actix_web::error::ErrorInternalServerError(
        "jwt secret not found",
      ))?;
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

  authorization_from_token(token, jwt_secret_data)
}

#[instrument(level = "trace", skip_all, err)]
pub fn authorization_from_token(
  token: &str,
  jwt_secret: &Data<Secret<String>>,
) -> Result<Authorization, actix_web::Error> {
  let claims = gotrue_jwt_claims_from_token(token, jwt_secret)?;
  Ok(Authorization {
    token: token.to_string(),
    claims,
  })
}

#[instrument(level = "trace", skip_all, err)]
fn gotrue_jwt_claims_from_token(
  token: &str,
  jwt_secret: &Data<Secret<String>>,
) -> Result<GoTrueJWTClaims, actix_web::Error> {
  let claims =
    GoTrueJWTClaims::decode(token, jwt_secret.expose_secret().as_bytes()).map_err(|err| {
      actix_web::error::ErrorUnauthorized(format!("fail to decode token, error:{}", err))
    })?;
  Ok(claims)
}
