use crate::component::auth::{
    compute_hash_password, internal_error, validate_credentials, AuthError, Credentials,
};
use crate::config::env::{domain, jwt_secret};
use crate::state::Cache;
use actix_web::http::header::HeaderValue;
use actix_web::{FromRequest, HttpRequest};
use anyhow::{Context, Error};
use chrono::Utc;
use chrono::{Duration, Local};
use futures_util::future::{ready, Ready};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use secrecy::Secret;
use serde::{Deserialize, Serialize};
use sqlx::types::uuid;
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

pub async fn login(
    pg_pool: PgPool,
    cache: Arc<RwLock<Cache>>,
    email: String,
    password: String,
) -> Result<LoginResponse, AuthError> {
    let credentials = Credentials {
        email,
        password: Secret::new(password),
    };

    match validate_credentials(credentials, &pg_pool).await? {
        Ok(uid) => {
            let uid = uid.to_string();
            let token = Token::create_token(&uid)?.into();
            let logged_user = LoggedUser::new(uid);
            cache.write().await.authorized(logged_user);

            Ok(LoginResponse {
                token,
                user_id: uid,
            })
        }
        Err(err) => Err(err),
    }
}

pub async fn logout(logged_user: LoggedUser, cache: Arc<RwLock<Cache>>) -> Result<(), AuthError> {
    cache.write().await.unauthorized(logged_user);
    Ok(())
}

pub async fn register(
    pg_pool: PgPool,
    cache: Arc<RwLock<Cache>>,
    username: String,
    email: String,
    password: String,
) -> Result<RegisterResponse, AuthError> {
    let mut transaction = pg_pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection to register user")
        .map_err(internal_error)?;

    if is_email_exist(&mut transaction, email.as_ref())
        .await
        .map_err(internal_error)?
    {
        return Err(AuthError::UserAlreadyExist { email });
    }

    let uuid = uuid::Uuid::new_v4();
    let token = Token::create_token(&uuid.to_string())?;
    let password = compute_hash_password(password.as_bytes()).map_err(internal_error)?;
    let _ = sqlx::query!(
        r#"
            INSERT INTO users (uid, email, username, create_time, password)
            VALUES ($1, $2, $3, $4, $5)
        "#,
        uuid,
        email,
        username,
        Utc::now(),
        password,
    )
    .execute(&mut transaction)
    .await
    .context("Save user to disk failed")
    .map_err(internal_error)?;

    transaction
        .commit()
        .await
        .context("Failed to commit SQL transaction to register user.")
        .map_err(internal_error)?;

    let logged_user = LoggedUser::new(uuid.to_string());
    cache.write().await.authorized(logged_user);

    Ok(RegisterResponse {
        token: token.into(),
    })
}

/// TODO: cache this state in [State]
async fn is_email_exist(
    transaction: &mut Transaction<'_, Postgres>,
    email: &str,
) -> Result<bool, anyhow::Error> {
    let result = sqlx::query(r#"SELECT email FROM users WHERE email = $1"#)
        .bind(email)
        .fetch_optional(transaction)
        .await?;

    Ok(result.is_some())
}

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
    pub name: String,
}

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct LoginResponse {
    pub token: String,
    pub user_id: String,
}

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct RegisterRequestParams {
    pub email: String,
    pub password: String,
    pub name: String,
}

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct RegisterResponse {
    pub token: String,
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Ord, PartialOrd)]
pub struct LoggedUser {
    pub uid: String,
}

impl std::convert::From<Claim> for LoggedUser {
    fn from(c: Claim) -> Self {
        Self { uid: c.user_id() }
    }
}

impl LoggedUser {
    pub fn new(user_id: String) -> Self {
        Self { uid: user_id }
    }

    pub fn from_token(token: String) -> Result<Self, AuthError> {
        let user: LoggedUser = Token::decode_token(&token.into())?.into();
        Ok(user)
    }

    pub fn as_uuid(&self) -> Result<uuid::Uuid, anyhow::Error> {
        let uuid = uuid::Uuid::parse_str(&self.uid).context("Invalid uuid")?;
        Ok(uuid)
    }
}

impl FromRequest for LoggedUser {
    type Error = AuthError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut actix_http::Payload) -> Self::Future {
        match Token::parser_from_request(req) {
            Ok(token) => ready(LoggedUser::from_token(token.0)),
            Err(err) => ready(Err(err)),
        }
    }
}

impl std::convert::TryFrom<&HeaderValue> for LoggedUser {
    type Error = AuthError;

    fn try_from(header: &HeaderValue) -> Result<Self, Self::Error> {
        match header.to_str() {
            Ok(val) => LoggedUser::from_token(val.to_owned()),
            Err(e) => {
                tracing::error!("Header to string failed: {:?}", e);
                Err(AuthError::Unauthorized)
            }
        }
    }
}

pub const HEADER_TOKEN: &str = "token";
const DEFAULT_ALGORITHM: Algorithm = Algorithm::HS256;
pub const EXPIRED_DURATION_DAYS: i64 = 30;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claim {
    // issuer
    iss: String,
    // subject
    sub: String,
    // issue at
    iat: i64,
    // expiry
    exp: i64,
    user_id: String,
}

impl Claim {
    pub fn with_user_id(user_id: &str) -> Self {
        let domain = domain();
        Self {
            iss: domain,
            sub: "auth".to_string(),
            user_id: user_id.to_string(),
            iat: Local::now().timestamp(),
            exp: (Local::now() + Duration::days(EXPIRED_DURATION_DAYS)).timestamp(),
        }
    }

    pub fn user_id(self) -> String {
        self.user_id
    }
}

#[derive(Clone)]
pub struct Token(pub String);
impl Token {
    pub fn create_token(user_id: &str) -> Result<Self, AuthError> {
        let claims = Claim::with_user_id(user_id);
        encode(
            &Header::new(DEFAULT_ALGORITHM),
            &claims,
            &EncodingKey::from_secret(jwt_secret().as_ref()),
        )
        .map(Into::into)
        .context("Create user token failed")
        .map_err(|err| AuthError::InternalError(err))
    }

    pub fn decode_token(token: &Self) -> Result<Claim, AuthError> {
        decode::<Claim>(
            &token.0,
            &DecodingKey::from_secret(jwt_secret().as_ref()),
            &Validation::new(DEFAULT_ALGORITHM),
        )
        .map(|data| Ok(data.claims))
        .map_err(|err| AuthError::Unauthorized)?
    }

    pub fn parser_from_request(request: &HttpRequest) -> Result<Self, AuthError> {
        match request.headers().get(HEADER_TOKEN) {
            Some(header) => match header.to_str() {
                Ok(val) => Ok(Token(val.to_owned())),
                Err(_) => Err(AuthError::Unauthorized),
            },
            None => Err(AuthError::Unauthorized),
        }
    }
}
impl std::convert::From<String> for Token {
    fn from(val: String) -> Self {
        Self(val)
    }
}

impl std::convert::Into<String> for Token {
    fn into(self) -> String {
        self.0
    }
}

impl FromRequest for Token {
    type Error = AuthError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, payload: &mut actix_http::Payload) -> Self::Future {
        match Token::parser_from_request(req) {
            Ok(token) => ready(Ok(token)),
            Err(err) => ready(Err(err)),
        }
    }
}
