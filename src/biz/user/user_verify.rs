use std::ops::DerefMut;

use anyhow::{Context, Result};
use sqlx::types::uuid;
use tracing::{event, instrument, trace};

use access_control::workspace::WorkspaceAccessControl;
use app_error::AppError;
use database::user::{create_user, is_user_exist};
use database::workspace::select_workspace;
use database_entity::dto::AFRole;
use workspace_template::document::get_started::GetStartedDocumentTemplate;

use crate::biz::user::user_init::initialize_workspace_for_user;
use crate::state::AppState;

/// Verify the token from the gotrue server and create the user if it is a new user
/// Return true if the user is a new user
///
#[instrument(skip_all, err)]
pub async fn verify_token(access_token: &str, state: &AppState) -> Result<bool, AppError> {
  let user = state.gotrue_client.user_info(access_token).await?;
  let user_uuid = uuid::Uuid::parse_str(&user.id)?;
  let name = name_from_user_metadata(&user.user_metadata);

  let is_new = !is_user_exist(&state.pg_pool, &user_uuid).await?;
  if !is_new {
    return Ok(false);
  }

  let mut txn = state
    .pg_pool
    .begin()
    .await
    .context("acquire transaction to verify token")?;

  // To prevent concurrent creation of the same user with the same workspace resources, we lock
  // the user row when `verify_token` is called. This means that if multiple requests try to
  // create the same user simultaneously, the first request will acquire the lock, create the user,
  // and any subsequent requests will wait for the lock to be released. After the lock is released,
  // the other requests will proceed and return the result, ensuring that each user is created only once
  // and avoiding duplicate entries.
  let lock_key = user_uuid.as_u128() as i64;
  sqlx::query!("SELECT pg_advisory_xact_lock($1)", lock_key)
    .execute(txn.deref_mut())
    .await?;

  let is_new = !is_user_exist(txn.deref_mut(), &user_uuid).await?;
  if is_new {
    let new_uid = state.id_gen.write().await.next_id();
    event!(tracing::Level::INFO, "create new user:{}", new_uid);
    let workspace_id =
      create_user(txn.deref_mut(), new_uid, &user_uuid, &user.email, &name).await?;
    let workspace_row = select_workspace(txn.deref_mut(), &workspace_id).await?;

    // It's essential to cache the user's role because subsequent actions will rely on this cached information.
    state
      .workspace_access_control
      .insert_role(&new_uid, &workspace_id, AFRole::Owner)
      .await?;

    // Create a workspace with the GetStarted template
    initialize_workspace_for_user(
      new_uid,
      &workspace_row,
      &mut txn,
      vec![GetStartedDocumentTemplate],
      &state.collab_access_control_storage,
    )
    .await?;
  } else {
    trace!("user already exists:{},{}", user.id, user.email);
  }
  txn
    .commit()
    .await
    .context("fail to commit transaction to verify token")?;
  Ok(is_new)
}

// Best effort to get user's name after oauth
fn name_from_user_metadata(value: &serde_json::Value) -> String {
  value
    .get("name")
    .or(value.get("full_name"))
    .or(value.get("nickname"))
    .and_then(serde_json::Value::as_str)
    .map(str::to_string)
    .unwrap_or_default()
}
