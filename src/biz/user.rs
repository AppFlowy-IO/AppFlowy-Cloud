use anyhow::Result;
use gotrue::{
  api::Client,
  grant::{Grant, PasswordGrant},
  models::{AccessTokenResponse, User},
};

use shared_entity::{error::AppError, server_error};
use validator::validate_email;

use crate::domain::validate_password;

pub async fn sign_up(gotrue_client: &Client, email: &str, password: &str) -> Result<(), AppError> {
  validate_email_password(email, password)?;

  let user = gotrue_client.sign_up(email, password).await??;
  tracing::info!("user: {:?}", user);

  // TODO: set up workspace for new user

  Ok(())
}

pub async fn sign_in(
  gotrue_client: &Client,
  email: String,
  password: String,
) -> Result<AccessTokenResponse, AppError> {
  let grant = Grant::Password(PasswordGrant { email, password });
  let token = gotrue_client.token(&grant).await??;
  Ok(token)
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
