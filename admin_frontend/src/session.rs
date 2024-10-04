use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
  async_trait,
  extract::{FromRequestParts, OriginalUri},
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

fn code_session_key(code: &str) -> String {
  format!("web::session::code::{}", code)
}

impl SessionStorage {
  pub fn new(redis_client: ConnectionManager) -> Self {
    Self { redis_client }
  }

  pub async fn get_user_session(
    &self,
    session_id: &str,
  ) -> Result<Option<UserSession>, redis::RedisError> {
    let key = session_id_key(session_id);
    let user_session_optional: UserSessionOptional = self.redis_client.clone().get(&key).await?;
    Ok(user_session_optional.0)
  }

  pub async fn get_code_session(
    &self,
    code: &str,
  ) -> Result<Option<CodeSession>, redis::RedisError> {
    let key = code_session_key(code);
    let code_session_optional: CodeSessionOptional = self.redis_client.clone().get(&key).await?;
    Ok(code_session_optional.0)
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

  pub async fn put_code_session(
    &self,
    code: &str,
    code_session: &CodeSession,
  ) -> redis::RedisResult<()> {
    let key = code_session_key(code);
    self
      .redis_client
      .clone()
      .set_options(
        key,
        code_session,
        redis::SetOptions::default().with_expiration(redis::SetExpiry::EX(60 * 5)), // code is valid for 5 minutes
      )
      .await
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CodeSession {
  pub session_id: String,
  pub code_challenge: Option<String>,
  pub code_challenge_method: Option<String>,
}
pub struct CodeSessionOptional(Option<CodeSession>);

impl ToRedisArgs for CodeSession {
  fn write_redis_args<W>(&self, out: &mut W)
  where
    W: ?Sized + redis::RedisWrite,
  {
    let s = serde_json::to_string(self).unwrap();
    out.write_arg(s.as_bytes());
  }
}

impl FromRedisValue for CodeSessionOptional {
  fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
    let bytes = expect_redis_value_data(v)?;
    match bytes {
      Some(bytes) => {
        let session = expect_redis_json_bytes(bytes).unwrap();
        Ok(CodeSessionOptional(Some(session)))
      },
      None => Ok(CodeSessionOptional(None)),
    }
  }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserSession {
  pub session_id: String,
  pub token: GotrueTokenResponse,
}
pub struct UserSessionOptional(Option<UserSession>);

#[async_trait]
impl FromRequestParts<AppState> for UserSession {
  type Rejection = SessionRejection;

  async fn from_request_parts(
    parts: &mut Parts,
    state: &AppState,
  ) -> Result<Self, Self::Rejection> {
    let mut cookie_jar = CookieJar::from_request_parts(parts, state)
      .await
      .map_err(|e| {
        tracing::error!("failed to get cookie jar");
        SessionRejection::new_no_cookie(SessionRejectionKind::CookieError(e.to_string()))
      })?;

    let session =
      match get_session_from_store(&cookie_jar, &state.session_store, &state.gotrue_client).await {
        Ok(session) => session,
        Err(err) => {
          if let Some(original_url) = parts.extensions.get::<OriginalUri>() {
            let mut pre_login_cookie = Cookie::new("pre_login_url", original_url.to_string());
            pre_login_cookie.set_path("/");
            cookie_jar = cookie_jar.add(pre_login_cookie);
          };
          return Err(SessionRejection::new(cookie_jar, err));
        },
      };
    Ok(session)
  }
}

async fn get_session_from_store(
  cookie_jar: &CookieJar,
  session_store: &SessionStorage,
  gotrue_client: &gotrue::api::Client,
) -> Result<UserSession, SessionRejectionKind> {
  let session_id = cookie_jar
    .get("session_id")
    .ok_or(SessionRejectionKind::NoSessionId)?
    .value();
  let mut session = session_store
    .get_user_session(session_id)
    .await
    .map_err(|err| {
      tracing::info!("failed to get session from store: {}", err);
      SessionRejectionKind::SessionNotFound
    })?
    .ok_or(SessionRejectionKind::SessionNotFound)?;

  if has_expired(session.token.access_token.as_str()) {
    // Get new pair of access token and refresh token
    let refresh_token = session.token.refresh_token;
    let new_token = gotrue_client
      .clone()
      .token(&Grant::RefreshToken(RefreshTokenGrant { refresh_token }))
      .await
      .map_err(|err| SessionRejectionKind::RefreshTokenError(err.to_string()))?;

    session.token.access_token = new_token.access_token;
    session.token.refresh_token = new_token.refresh_token;

    // Update session in redis
    session_store
      .put_user_session(&session)
      .await
      .unwrap_or_else(|err| tracing::error!("failed to update session: {}", err));
  }

  Ok(session)
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
pub struct SessionRejection {
  pub cookie_jar: CookieJar,
  pub session_rejection: SessionRejectionKind,
}

impl SessionRejection {
  fn new_no_cookie(kind: SessionRejectionKind) -> Self {
    Self {
      cookie_jar: CookieJar::new(),
      session_rejection: kind,
    }
  }

  fn new(cookie_jar: CookieJar, kind: SessionRejectionKind) -> Self {
    Self {
      cookie_jar,
      session_rejection: kind,
    }
  }
}

impl IntoResponse for SessionRejection {
  fn into_response(self) -> axum::response::Response {
    tracing::info!("session rejection: {:?}", self);
    match self.session_rejection {
      SessionRejectionKind::Redirect(url) => {
        (self.cookie_jar, Redirect::temporary(&url)).into_response()
      },
      x => {
        tracing::info!("session rejection: {:?}", x);
        (self.cookie_jar, Redirect::temporary("/web/login")).into_response()
      },
    }
  }
}

#[derive(Clone, Debug)]
pub enum SessionRejectionKind {
  NoSessionId,
  SessionNotFound,
  CookieError(String),
  RefreshTokenError(String),
  Redirect(String),
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

impl FromRedisValue for UserSessionOptional {
  fn from_redis_value(v: &redis::Value) -> redis::RedisResult<Self> {
    let bytes = expect_redis_value_data(v)?;
    match bytes {
      Some(bytes) => {
        let session = expect_redis_json_bytes(bytes).unwrap();
        Ok(UserSessionOptional(Some(session)))
      },
      None => Ok(UserSessionOptional(None)),
    }
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

fn expect_redis_value_data(v: &redis::Value) -> redis::RedisResult<Option<&[u8]>> {
  match v {
    redis::Value::Data(ref bytes) => Ok(Some(bytes)),
    redis::Value::Nil => Ok(None),
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
