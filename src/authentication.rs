use actix_http::Payload;
use actix_web::{FromRequest, HttpRequest};
use authentication::jwt::Authorization;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Debug)]
pub struct UserIdentity {
  pub user_uuid: Uuid,
  pub source: UserIdentitySource,
}

impl UserIdentity {
  pub fn from_auth(auth: Authorization) -> Result<Self, actix_web::Error> {
    Ok(Self {
      user_uuid: auth.uuid()?,
      source: UserIdentitySource::Jwt(auth),
    })
  }

  pub async fn from_api_key(
    api_key: &str,
    pg_pool: &sqlx::PgPool,
  ) -> Result<Self, actix_web::Error> {
    todo!()
  }

  pub fn from_actix_request(req: &HttpRequest) -> Result<Self, actix_web::Error> {
    let api_key = req
      .headers()
      .get("x-api-key")
      .and_then(|header| header.to_str().ok().map(|s| s.to_string()));
    match api_key {
      Some(api_key) => {
        let pg_pool = &req
          .app_data::<AppState>()
          .ok_or(actix_web::error::ErrorInternalServerError(
            "App state not found",
          ))?
          .pg_pool;
        futures::executor::block_on(UserIdentity::from_api_key(&api_key, pg_pool))
      },
      None => {
        let auth = Authorization::from_actix_request(req)?;
        UserIdentity::from_auth(auth)
      },
    }
  }
}

#[derive(Debug)]
pub enum UserIdentitySource {
  Jwt(Authorization),
  ApiKey(String),
}

impl FromRequest for UserIdentity {
  type Error = actix_web::Error;

  type Future = std::future::Ready<Result<Self, Self::Error>>;

  fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
    std::future::ready(UserIdentity::from_actix_request(req))
  }
}
