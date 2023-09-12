use std::str::FromStr;

use anyhow::Result;
use gotrue::{
  api::Client,
  grant::{Grant, PasswordGrant},
  models::{AccessTokenResponse, User},
};

use shared_entity::{error::AppError, server_error};
use validator::validate_email;

use crate::domain::validate_password;
use sqlx::{types::uuid, PgPool};
use tracing::instrument;

#[instrument(level = "info", skip_all, err)]
pub async fn sign_up(
  gotrue_client: &Client,
  email: &str,
  password: &str,
  pg_pool: &PgPool,
) -> Result<i64, AppError> {
  validate_email_password(email, password)?;
  let user = gotrue_client.sign_up(email, password).await??;
  tracing::info!("user sign up: {:?}", user);
  let gotrue_uuid = uuid::Uuid::from_str(&user.id)?;
  storage::workspace::create_user_if_not_exists(pg_pool, &gotrue_uuid, &user.email).await?;
  // let uid = storage::workspace::get_user_id(pg_pool, &gotrue_uuid).await?;
  Ok(1)
}

#[instrument(level = "info", skip_all, err)]
pub async fn sign_in(
  pg_pool: &PgPool,
  gotrue_client: &Client,
  email: String,
  password: String,
) -> Result<(AccessTokenResponse, i64), AppError> {
  let grant = Grant::Password(PasswordGrant { email, password });
  let token = gotrue_client.token(&grant).await??;

  let gotrue_uuid = uuid::Uuid::from_str(&token.user.id)?;
  storage::workspace::create_user_if_not_exists(pg_pool, &gotrue_uuid, &token.user.email).await?;
  // let uid = storage::workspace::get_user_id(pg_pool, &gotrue_uuid).await?;
  Ok((token, 1))
}

pub async fn update(
  gotrue_client: &Client,
  token: &str,
  email: &str,
  password: &str,
) -> Result<User, AppError> {
  validate_email_password(email, password)?;
  let user = gotrue_client.update_user(token, email, password).await??;
  Ok(user)
}

fn validate_email_password(email: &str, password: &str) -> Result<(), AppError> {
  if !validate_email(email) {
    Err(server_error::invalid_email_error(email))
  } else if !validate_password(password) {
    Err(server_error::invalid_password_error(password))
  } else {
    Ok(())
  }
}
