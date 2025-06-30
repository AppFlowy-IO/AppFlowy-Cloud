use crate::biz::authentication::jwt::Authorization;
use crate::state::GoTrueAdmin;
use crate::{biz::workspace::ops::delete_workspace_for_user, config::config::AppleOAuthSetting};
use app_error::ErrorCode;
use database::file::s3_client_impl::S3BucketStorage;
use database::workspace::{insert_workspace_ids_to_deleted_table, select_user_owned_workspaces_id};
use gotrue::params::AdminDeleteUserParams;
use redis::aio::ConnectionManager;
use secrecy::{ExposeSecret, Secret};
use shared_entity::response::AppResponseError;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

#[allow(clippy::too_many_arguments)]
pub async fn delete_user(
  pg_pool: &sqlx::PgPool,
  connection_manager: &ConnectionManager,
  bucket_storage: &Arc<S3BucketStorage>,
  gotrue_client: &gotrue::api::Client,
  gotrue_admin: &GoTrueAdmin,
  apple_oauth: &AppleOAuthSetting,
  auth: Authorization,
  user_uuid: Uuid,
  provider_access_token: Option<String>,
  provider_refresh_token: Option<String>,
) -> Result<(), AppResponseError> {
  if is_apple_user(&auth) {
    if let Err(err) = revoke_apple_user(
      &apple_oauth.client_id,
      &apple_oauth.client_secret,
      provider_access_token,
      provider_refresh_token,
    )
    .await
    {
      tracing::warn!("revoke apple user failed: {:?}", err);
    };
  }

  info!("admin deleting user: {:?}", user_uuid);
  let admin_token = gotrue_admin.token().await?;
  gotrue_client
    .admin_delete_user(
      &admin_token,
      &user_uuid.to_string(),
      &AdminDeleteUserParams {
        should_soft_delete: false,
      },
    )
    .await
    .map_err(AppResponseError::from)?;

  // spawn tasks to delete all workspaces owned by the user
  let workspace_ids = select_user_owned_workspaces_id(pg_pool, &user_uuid).await?;

  info!(
    "saving workspaces: {:?} to deleted workspace table",
    workspace_ids
  );
  insert_workspace_ids_to_deleted_table(pg_pool, workspace_ids.clone()).await?;

  let mut tasks = vec![];
  for workspace_id in workspace_ids {
    let cloned_pg_pool = pg_pool.clone();
    let connection_manager = connection_manager.clone();
    tasks.push(tokio::spawn(delete_workspace_for_user(
      cloned_pg_pool,
      connection_manager,
      workspace_id,
      bucket_storage.clone(),
    )));
  }
  for task in tasks {
    task.await??;
  }

  Ok(())
}

async fn revoke_apple_user(
  client_id: &str,
  client_secret: &Secret<String>,
  apple_access_token: Option<String>,
  apple_refresh_token: Option<String>,
) -> Result<(), AppResponseError> {
  let (type_type_hint, token) = match apple_access_token {
    Some(access_token) => ("access_token", access_token),
    None => match apple_refresh_token {
      Some(refresh_token) => ("refresh_token", refresh_token),
      None => {
        return Err(AppResponseError::new(
          ErrorCode::InvalidRequest,
          "apple email deletion must provide access_token or refresh_token",
        ))
      },
    },
  };

  if let Err(err) = revoke_apple_token_http_call(
    client_id,
    client_secret.expose_secret(),
    &token,
    type_type_hint,
  )
  .await
  {
    tracing::warn!("revoke apple token failed: {:?}", err);
  };
  Ok(())
}

fn is_apple_user(auth: &Authorization) -> bool {
  if let Some(provider) = auth.claims.app_metadata.get("provider") {
    if provider == "apple" {
      return true;
    }
  };

  if let Some(providers) = auth.claims.app_metadata.get("providers") {
    if let Some(providers) = providers.as_array() {
      for provider in providers {
        if provider == "apple" {
          return true;
        }
      }
    }
  }

  false
}

/// Based on: https://developer.apple.com/documentation/sign_in_with_apple/revoke_tokens
async fn revoke_apple_token_http_call(
  apple_client_id: &str,
  apple_client_secret: &str,
  apple_user_token: &str,
  token_type_hint: &str,
) -> Result<(), AppResponseError> {
  let resp = reqwest::Client::new()
    .post("https://appleid.apple.com/auth/revoke")
    .form(&[
      ("client_id", apple_client_id),
      ("client_secret", apple_client_secret),
      ("token", apple_user_token),
      ("token_type_hint", token_type_hint),
    ])
    .send()
    .await?;

  let status = resp.status();
  if status.is_success() {
    return Ok(());
  }

  let payload = resp.text().await?;
  Err(AppResponseError::new(
    ErrorCode::AppleRevokeTokenError,
    format!(
      "calling apple revoke, code: {}, message: {}",
      status, payload
    ),
  ))
}
