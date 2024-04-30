use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
  async_trait,
  extract::FromRequestParts,
  http::request::Parts,
  response::{IntoResponse, Redirect},
};
use axum_extra::extract::{cookie::Cookie, CookieJar};
use gotrue::grant::{Grant, RefreshTokenGrant};
use gotrue_entity::dto::GotrueTokenResponse;
use jwt::{Claims, Header};
use redis::{aio::ConnectionManager, AsyncCommands, FromRedisValue, ToRedisArgs};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::AppState;

static SESSION_EXPIRATION: usize = 60 * 60 * 24; // 1 day

#[derive(Clone)]
pub struct SessionStorage {
  redis_client: ConnectionManager,
}

fn session_id_key(session_id: &str) -> String {
  format!("web::session::{}", session_id)
}

impl SessionStorage {
  pub fn new(redis_client: ConnectionManager) -> Self {
    Self { redis_client }
  }

  pub async fn get_user_session(&self, session_id: &str) -> Option<UserSession> {
    let key = session_id_key(session_id);
    let s: Result<UserSession, redis::RedisError> = self.redis_client.clone().get(&key).await;
    match s {
      Ok(s) => Some(s),
      Err(e) => {
        tracing::info!("get user session in redis error: {:?}", e);
        None
      },
    }
  }

  pub async fn put_user_session(&self, user_session: &UserSession) -> redis::RedisResult<()> {
    let key = session_id_key(&user_session.session_id);
    self
      .redis_client
      .clone()
      .set_options(
        key,
        user_session,
        redis::SetOptions::default().with_expiration(redis::SetExpiry::EX(SESSION_EXPIRATION)),
      )
      .await
  }

  pub async fn del_user_session(&self, session_id: &str) -> redis::RedisResult<()> {
    let key = session_id_key(session_id);
    let res = self.redis_client.clone().del::<_, i64>(key).await?;
    tracing::info!("del user session: {} res: {}", session_id, res);
    Ok(())
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserSession {
  pub session_id: String,
  pub token: GotrueTokenResponse,
}

impl UserSession {
  pub fn new(session_id: String, token: GotrueTokenResponse) -> Self {
    Self { session_id, token }
  }
}

#[async_trait]
impl FromRequestParts<AppState> for UserSession {
  type Rejection = SessionRejection;

  async fn from_request_parts(
    parts: &mut Parts,
    state: &AppState,
  ) -> Result<Self, Self::Rejection> {
    let jar = CookieJar::from_request_parts(parts, state)
      .await
      .map_err(|e| SessionRejection::CookieError(e.to_string()))?;

    let session_id = jar
      .get("session_id")
      .ok_or(SessionRejection::NoSessionId)?
      .value();

    let mut session = state
      .session_store
      .get_user_session(session_id)
      .await
      .ok_or(SessionRejection::SessionNotFound)?;

    if has_expired(session.token.access_token.as_str()) {
      // Get new pair of access token and refresh token
      let refresh_token = session.token.refresh_token;
      let new_token = state
        .gotrue_client
        .clone()
        .token(&Grant::RefreshToken(RefreshTokenGrant { refresh_token }))
        .await
        .map_err(|err| SessionRejection::RefreshTokenError(err.to_string()))?;

      session.token.access_token = new_token.access_token;
      session.token.refresh_token = new_token.refresh_token;

      // Update session in redis
      let _ = state
        .session_store
        .put_user_session(&session)
        .await
        .map_err(|err| {
          tracing::error!("failed to update session in redis: {}", err);
        });
    }

    Ok(session)
  }
}

fn has_expired(access_token: &str) -> bool {
  match get_session_expiration(access_token) {
    Some(expiration_seconds) => {
      let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs();
      now > expiration_seconds
    },
    None => false,
  }
}

fn get_session_expiration(access_token: &str) -> Option<u64> {
  // no need to verify, let the appflowy cloud server do it
  // in that way, frontend server does not need to know the secret
  match jwt::Token::<Header, Claims, _>::parse_unverified(access_token) {
    Ok(unverified) => unverified.claims().registered.expiration,
    Err(e) => {
      tracing::error!("failed to parse unverified token: {}", e);
      None
    },
  }
}

#[derive(Clone, Debug)]
pub enum SessionRejection {
  NoSessionId,
  SessionNotFound,
  CookieError(String),
  RefreshTokenError(String),
}

impl IntoResponse for SessionRejection {
  fn into_response(self) -> axum::response::Response {
    match self {
      SessionRejection::NoSessionId => Redirect::temporary("/web/login").into_response(),
      SessionRejection::CookieError(err) => {
        tracing::error!("session rejection cookie error: {}", err);
        Redirect::temporary("/web/login").into_response()
      },
      SessionRejection::SessionNotFound => Redirect::temporary("/web/login").into_response(),
      SessionRejection::RefreshTokenError(err) => {
        tracing::warn!("refresh token error: {}", err);
        Redirect::temporary("/web/login").into_response()
      },
    }
  }
}

impl ToRedisArgs for UserSession {
  fn write_redis_args<W>(&self, out: &mut W)
  where
    W: ?Sized + redis::RedisWrite,
  {
    let s = serde_json::to_string(self).unwrap();
    out.write_arg(s.as_bytes());
  }
}

impl FromRedisValue for UserSession {
  fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
    let bytes = expect_redis_value_data(v)?;
    expect_redis_json_bytes(bytes)
  }
}

fn expect_redis_json_bytes<T>(v: &[u8]) -> redis::RedisResult<T>
where
  T: DeserializeOwned,
{
  let res: Result<T, serde_json::Error> = serde_json::from_slice(v);
  match res {
    Ok(v) => Ok(v),
    Err(e) => Err(redis::RedisError::from((
      redis::ErrorKind::TypeError,
      "redis data json deserialization failed!",
      e.to_string(),
    ))),
  }
}

fn expect_redis_value_data(v: &redis::Value) -> redis::RedisResult<&[u8]> {
  match v {
    redis::Value::Data(ref bytes) => Ok(bytes),
    x => Err(redis::RedisError::from((
      redis::ErrorKind::TypeError,
      "unexpected value from redis",
      format!("redis value is not data: {:?}", x),
    ))),
  }
}

pub fn new_session_cookie(id: uuid::Uuid) -> Cookie<'static> {
  let mut cookie = Cookie::new("session_id", id.to_string());
  cookie.set_path("/");
  cookie
}
