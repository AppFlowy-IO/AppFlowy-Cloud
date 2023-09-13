use std::str::FromStr;

use anyhow::Result;
use gotrue::{
  api::Client,
  grant::{Grant, PasswordGrant},
};
use gotrue_entity::{AccessTokenResponse, OAuthProvider, OAuthURL, User};
use shared_entity::{
  error::AppError,
  error_code::{invalid_email_error, invalid_password_error, ErrorCode},
};
use storage_entity::{AFUserProfileView, AFWorkspaces};
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
) -> Result<(), AppError> {
  validate_email_password(email, password)?;
  let user = gotrue_client.sign_up(email, password).await??;
  tracing::info!("user sign up: {:?}", user);
  if user.confirmed_at.is_some() {
    let gotrue_uuid = uuid::Uuid::from_str(&user.id)?;
    storage::workspace::create_user_if_not_exists(pg_pool, &gotrue_uuid, &user.email).await?;
  }
  Ok(())
}

pub async fn info(gotrue_client: &Client, access_token: &str) -> Result<User, AppError> {
  Ok(gotrue_client.user_info(access_token).await??)
}

pub async fn oauth(gotrue_client: &Client, provider: OAuthProvider) -> Result<OAuthURL, AppError> {
  let settings = gotrue_client.settings().await?;
  if settings.external.has_provider(&provider) {
    Ok(OAuthURL {
      url: format!(
        "{}/authorize?provider={}",
        gotrue_client.base_url,
        provider.as_str(),
      ),
    })
  } else {
    Err(ErrorCode::InvalidOAuthProvider.into())
  }
}

pub async fn user_workspaces(
  pg_pool: &PgPool,
  uuid: &uuid::Uuid,
) -> Result<AFWorkspaces, AppError> {
  let workspaces = storage::workspace::select_all_workspaces_owned(pg_pool, uuid).await?;
  Ok(AFWorkspaces(workspaces))
}

pub async fn user_profile(
  pg_pool: &PgPool,
  uuid: &uuid::Uuid,
) -> Result<AFUserProfileView, AppError> {
  let profile = storage::workspace::select_user_profile_view_by_uuid(pg_pool, uuid)
    .await?
    .ok_or(sqlx::Error::RowNotFound)?;
  Ok(profile)
}

#[instrument(level = "info", skip_all, err)]
pub async fn sign_in(
  pg_pool: &PgPool,
  gotrue_client: &Client,
  email: String,
  password: String,
) -> Result<AccessTokenResponse, AppError> {
  let grant = Grant::Password(PasswordGrant { email, password });
  let token = gotrue_client.token(&grant).await??;

  let gotrue_uuid = uuid::Uuid::from_str(&token.user.id)?;
  storage::workspace::create_user_if_not_exists(pg_pool, &gotrue_uuid, &token.user.email).await?;
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
    Err(invalid_email_error(email))
  } else if !validate_password(password) {
    Err(invalid_password_error(password))
  } else {
    Ok(())
  }
}
