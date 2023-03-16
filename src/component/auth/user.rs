use crate::component::auth::{
    compute_hash_password, internal_error, validate_credentials, AuthError, Credentials,
};
use crate::config::env::domain;
use crate::state::{State, UserCache};
use crate::telemetry::spawn_blocking_with_tracing;
use actix_web::HttpRequest;
use anyhow::Context;
use chrono::Duration;
use chrono::Utc;

use secrecy::zeroize::DefaultIsZeroes;
use secrecy::{CloneableSecret, DebugSecret, ExposeSecret, Secret, Zeroize};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;
use token::{create_token, parse_token, TokenError};
use tokio::sync::RwLock;

pub async fn login(
    email: String,
    password: String,
    state: &State,
) -> Result<(LoginResponse, Secret<Token>), AuthError> {
    let credentials = Credentials {
        email,
        password: Secret::new(password),
    };
    let server_key = &state.config.application.server_key;

    match validate_credentials(credentials, &state.pg_pool).await {
        Ok(uid) => {
            let token = Token::create_token(uid, server_key)?;
            let logged_user = LoggedUser::new(uid);
            state.user.write().await.authorized(logged_user);
            Ok((
                LoginResponse {
                    token: token.clone().into(),
                    uid: uid.to_string(),
                },
                Secret::new(token),
            ))
        }
        Err(err) => Err(err),
    }
}

pub async fn logout(logged_user: LoggedUser, cache: Arc<RwLock<UserCache>>) {
    cache.write().await.unauthorized(logged_user);
}

pub async fn register(
    username: String,
    email: String,
    password: String,
    state: &State,
) -> Result<RegisterResponse, AuthError> {
    let pg_pool = state.pg_pool.clone();
    let server_key = &state.config.application.server_key;
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

    let uid = state.id_gen.write().await.next_id();
    let token = Token::create_token(uid, server_key)?;
    let password = compute_hash_password(password.as_bytes()).map_err(internal_error)?;
    let _ = sqlx::query!(
        r#"
            INSERT INTO users (uid, email, username, create_time, password)
            VALUES ($1, $2, $3, $4, $5)
        "#,
        uid,
        email,
        username,
        Utc::now(),
        password.expose_secret(),
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

    let logged_user = LoggedUser::new(uid);
    state.user.write().await.authorized(logged_user);

    Ok(RegisterResponse {
        token: token.into(),
    })
}

pub async fn change_password(
    pg_pool: PgPool,
    logged_user: LoggedUser,
    current_password: String,
    new_password: String,
) -> Result<(), AuthError> {
    let mut transaction = pg_pool
        .begin()
        .await
        .context("Failed to acquire a Postgres connection to change password")
        .map_err(internal_error)?;

    let email = get_user_email(*logged_user.expose_secret(), &mut transaction).await?;

    // check password
    let credentials = Credentials {
        email,
        password: Secret::new(current_password),
    };
    let _ = validate_credentials(credentials, &pg_pool).await?;

    // Hash password
    let new_hash_password =
        spawn_blocking_with_tracing(move || compute_hash_password(new_password.as_bytes()))
            .await
            .context("Failed to hash password")??;

    // Save password to disk
    let sql = "UPDATE users SET password = $1 where uid = $2";
    let _ = sqlx::query(sql)
        .bind(new_hash_password.expose_secret())
        .bind(logged_user.expose_secret())
        .execute(&mut transaction)
        .await
        .context("Failed to change user's password in the database.")?;

    transaction
        .commit()
        .await
        .context("Failed to commit SQL transaction to change user's password")
        .map_err(internal_error)?;
    Ok(())
}

pub async fn get_user_email(
    uid: i64,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<String, anyhow::Error> {
    let row = sqlx::query!(
        r#"
        SELECT email 
        FROM users
        WHERE uid = $1
        "#,
        uid,
    )
    .fetch_one(transaction)
    .await
    .context("Failed to retrieve the username`")?;
    Ok(row.email)
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

#[derive(Default, Deserialize, Debug)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct LoginResponse {
    pub token: String,
    pub uid: String,
}

#[derive(Default, Deserialize, Debug)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub name: String,
}

#[derive(Default, Serialize, Deserialize, Debug)]
pub struct RegisterResponse {
    pub token: String,
}

#[derive(Default, Deserialize, Debug)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
    pub new_password_confirm: String,
}

#[derive(Clone)]
pub struct WrapI64(i64);
impl Default for WrapI64 {
    fn default() -> Self {
        Self(0)
    }
}
impl Copy for WrapI64 {}
impl DefaultIsZeroes for WrapI64 {}
impl DebugSecret for WrapI64 {}
impl CloneableSecret for WrapI64 {}

impl std::ops::Deref for WrapI64 {
    type Target = i64;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct LoggedUser(Secret<WrapI64>);

impl From<Claim> for LoggedUser {
    fn from(c: Claim) -> Self {
        Self(Secret::new(WrapI64(c.uid)))
    }
}

impl LoggedUser {
    pub fn new(uid: i64) -> Self {
        Self(Secret::new(WrapI64(uid)))
    }

    pub fn from_token(server_key: &Secret<String>, token: &str) -> Result<Self, AuthError> {
        let user: LoggedUser = Token::decode_token(server_key, token)?.into();
        Ok(user)
    }

    pub fn expose_secret(&self) -> &i64 {
        self.0.expose_secret()
    }
}

pub const HEADER_TOKEN: &str = "token";
pub const EXPIRED_DURATION_DAYS: i64 = 30;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claim {
    iss: String,
    uid: i64,
}

impl Claim {
    pub fn with_user_id(uid: i64) -> Self {
        Self { iss: domain(), uid }
    }
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Token(pub String);

impl Zeroize for Token {
    fn zeroize(&mut self) {
        self.0.zeroize()
    }
}

impl Token {
    pub fn create_token(uid: i64, server_key: &Secret<String>) -> Result<Self, AuthError> {
        let claim = Claim::with_user_id(uid);
        let token = create_token(
            server_key.expose_secret().as_str(),
            claim,
            Duration::days(EXPIRED_DURATION_DAYS),
        )
        .map_err(|e| match e {
            TokenError::Jwt(_) => AuthError::Unauthorized,
            TokenError::Expired => AuthError::Unauthorized,
        })?;
        Ok(Self(token))
    }

    pub fn decode_token(server_key: &Secret<String>, token: &str) -> Result<Claim, AuthError> {
        parse_token::<Claim>(server_key.expose_secret().as_str(), token)
            .map_err(|_| AuthError::Unauthorized)
    }
}

pub fn logged_user_from_request(
    request: &HttpRequest,
    server_key: &Secret<String>,
) -> Result<LoggedUser, AuthError> {
    match request.headers().get(HEADER_TOKEN) {
        None => Err(AuthError::Unauthorized),
        Some(header) => match header.to_str() {
            Ok(token_str) => LoggedUser::from_token(server_key, token_str),
            Err(_) => Err(AuthError::Unauthorized),
        },
    }
}

pub fn uid_from_request(
    request: &HttpRequest,
    server_key: &Secret<String>,
) -> Result<Secret<i64>, AuthError> {
    match request.headers().get(HEADER_TOKEN) {
        Some(header) => match header.to_str() {
            Ok(val) => Token::decode_token(server_key, val).map(|claim| Secret::new(claim.uid)),
            Err(_) => Err(AuthError::Unauthorized),
        },
        None => Err(AuthError::Unauthorized),
    }
}

impl From<String> for Token {
    fn from(val: String) -> Self {
        Self(val)
    }
}

impl Into<String> for Token {
    fn into(self) -> String {
        self.0
    }
}
