use std::str::FromStr;

use anyhow::Result;
use gotrue::{
  api::Client,
  grant::{Grant, PasswordGrant, RefreshTokenGrant},
};
use gotrue_entity::{AccessTokenResponse, OAuthProvider, OAuthURL, User};
use shared_entity::{
  error::AppError,
  error_code::{invalid_email_error, invalid_password_error, ErrorCode},
};
use storage_entity::AFUserProfileView;
use validator::validate_email;

use crate::domain::validate_password;
use sqlx::{types::uuid, PgPool};
use tracing::instrument;

pub async fn refresh(
  gotrue_client: &Client,
  refresh_token: String,
) -> Result<AccessTokenResponse, AppError> {
  let grant = Grant::RefreshToken(RefreshTokenGrant { refresh_token });
  let token = gotrue_client.token(&grant).await??;
  Ok(token)
}

#[instrument(level = "info", skip_all, err)]
pub async fn sign_up(
  gotrue_client: &Client,
  email: String,
  password: String,
) -> Result<(), AppError> {
  validate_email_password(&email, &password)?;
  let user = gotrue_client.sign_up(&email, &password).await??;
  tracing::info!("user sign up: {:?}", user);
  Ok(())
}

pub async fn sign_in_token(
  pg_pool: &PgPool,
  gotrue_client: &Client,
  access_token: &str,
) -> Result<(User, bool), AppError> {
  let user = gotrue_client.user_info(access_token).await??;
  let user_uuid = uuid::Uuid::from_str(&user.id)?;
  let name: String = name_from_user_metadata(&user.user_metadata);
  let new =
    storage::workspace::create_user_if_not_exists(pg_pool, &user_uuid, &user.email, &name).await?;
  Ok((user, new))
}

pub async fn oauth(gotrue_client: &Client, provider: OAuthProvider) -> Result<OAuthURL, AppError> {
  let settings = gotrue_client.settings().await?;
  if settings.external.has_provider(&provider) {
    Ok(OAuthURL {
      url: format!(
        "{}/authorize?provider={}",
        gotrue_client.ext_url,
        provider.as_str(),
      ),
    })
  } else {
    Err(ErrorCode::InvalidOAuthProvider.into())
  }
}

pub async fn get_profile(
  pg_pool: &PgPool,
  uuid: &uuid::Uuid,
) -> Result<AFUserProfileView, AppError> {
  let profile = storage::workspace::select_user_profile_view_by_uuid(pg_pool, uuid)
    .await?
    .ok_or(sqlx::Error::RowNotFound)?;
  Ok(profile)
}

#[instrument(level = "info", skip_all, err)]
pub async fn sign_in_password(
  pg_pool: &PgPool,
  gotrue_client: &Client,
  email: String,
  password: String,
) -> Result<(AccessTokenResponse, bool), AppError> {
  let grant = Grant::Password(PasswordGrant { email, password });
  let token = gotrue_client.token(&grant).await??;
  let gotrue_uuid = uuid::Uuid::from_str(&token.user.id)?;
  let new =
    storage::workspace::create_user_if_not_exists(pg_pool, &gotrue_uuid, &token.user.email, "")
      .await?;
  Ok((token, new))
}

pub async fn update(
  pg_pool: &PgPool,
  gotrue_client: &Client,
  token: &str,
  email: &str,
  password: &str,
  name: Option<&str>,
) -> Result<User, AppError> {
  validate_email_password(email, password)?;
  let user = gotrue_client.update_user(token, email, password).await??;
  let user_uuid = user.id.parse::<uuid::Uuid>()?;
  if let Some(name) = name {
    storage::workspace::update_user_name(pg_pool, &user_uuid, name).await?;
  }
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

// Best effort to get user's name after oauth
fn name_from_user_metadata(value: &serde_json::Value) -> String {
  value
    .get("name")
    .or(value.get("full_name"))
    .or(value.get("nickname"))
    .and_then(serde_json::Value::as_str)
    .map(str::to_string)
    .unwrap_or(String::new())
}
