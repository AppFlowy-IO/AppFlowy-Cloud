use crate::component::auth::{
    compute_hash_password, internal_error, validate_credentials, AuthError, Credentials,
};
use crate::config::env::domain;
use crate::state::UserCache;
use crate::telemetry::spawn_blocking_with_tracing;
use actix_web::HttpRequest;
use anyhow::Context;
use chrono::Duration;
use chrono::Utc;

use secrecy::{ExposeSecret, Secret, Zeroize};
use serde::{Deserialize, Serialize};
use sqlx::types::uuid;
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;
use token::{create_token, parse_token, TokenError};
use tokio::sync::RwLock;

pub async fn login(
    pg_pool: PgPool,
    cache: Arc<RwLock<UserCache>>,
    email: String,
    password: String,
    server_key: &Secret<String>,
) -> Result<(LoginResponse, Secret<Token>), AuthError> {
    let credentials = Credentials {
        email,
        password: Secret::new(password),
    };

    match validate_credentials(credentials, &pg_pool).await {
        Ok(uid) => {
            let uid = uid.to_string();
            let token = Token::create_token(&uid, server_key)?;
            let logged_user = LoggedUser::new(uid.clone());
            cache.write().await.authorized(logged_user);
            Ok((
                LoginResponse {
                    token: token.clone().into(),
                    uid,
                },
                Secret::new(token),
            ))
        }
        Err(err) => {
            //
            Err(err)
        }
    }
}

pub async fn logout(logged_user: LoggedUser, cache: Arc<RwLock<UserCache>>) {
    cache.write().await.unauthorized(logged_user);
}

pub async fn register(
    pg_pool: PgPool,
    cache: Arc<RwLock<UserCache>>,
    username: String,
    email: String,
    password: String,
    server_key: &Secret<String>,
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
    let token = Token::create_token(&uuid.to_string(), server_key)?;
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

    let logged_user = LoggedUser::new(uuid.to_string());
    cache.write().await.authorized(logged_user);

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

    let email = get_user_email(logged_user.expose_secret(), &mut transaction).await?;

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

    let uid =
        uuid::Uuid::parse_str(logged_user.expose_secret()).map_err(|e| AuthError::InvalidUuid {
            err: format!("{}", e),
        })?;
    // Save password to disk
    let sql = "UPDATE users SET password = $1 where uid = $2";
    let _ = sqlx::query(sql)
        .bind(new_hash_password.expose_secret())
        .bind(uid)
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
    uid: &str,
    transaction: &mut Transaction<'_, Postgres>,
) -> Result<String, anyhow::Error> {
    let uid = uuid::Uuid::parse_str(uid)?;
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

#[derive(Debug, Clone)]
pub struct LoggedUser {
    uid: Secret<String>,
}

impl From<Claim> for LoggedUser {
    fn from(c: Claim) -> Self {
        Self {
            uid: Secret::new(c.uid),
        }
    }
}

impl LoggedUser {
    pub fn new(uid: String) -> Self {
        Self {
            uid: Secret::new(uid),
        }
    }

    pub fn from_token(server_key: &Secret<String>, token: &str) -> Result<Self, AuthError> {
        let user: LoggedUser = Token::decode_token(server_key, token)?.into();
        Ok(user)
    }
}

impl std::ops::Deref for LoggedUser {
    type Target = Secret<String>;

    fn deref(&self) -> &Self::Target {
        &self.uid
    }
}

pub const HEADER_TOKEN: &str = "token";
pub const EXPIRED_DURATION_DAYS: i64 = 30;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claim {
    iss: String,
    uid: String,
}

impl Claim {
    pub fn with_user_id(uid: &str) -> Self {
        Self {
            iss: domain(),
            uid: uid.to_string(),
        }
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
    pub fn create_token(user_id: &str, server_key: &Secret<String>) -> Result<Self, AuthError> {
        let claim = Claim::with_user_id(user_id);
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
) -> Result<Secret<String>, AuthError> {
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
