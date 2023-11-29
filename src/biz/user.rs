use anyhow::{Context, Result};
use gotrue::api::Client;

use serde_json::json;
use shared_entity::response::AppResponseError;
use std::fmt::{Display, Formatter};
use std::ops::DerefMut;
use std::sync::Arc;
use uuid::Uuid;

use database::workspace::{select_user_profile, select_user_workspace, select_workspace};
use database_entity::dto::{AFUserProfile, AFUserWorkspaceInfo, AFWorkspace, InsertCollabParams};

use app_error::AppError;
use database::collab::insert_into_af_collab;
use database::user::{create_user, is_user_exist};
use database_entity::pg_row::AFUserNotification;
use realtime::entities::RealtimeUser;
use shared_entity::dto::auth_dto::UpdateUserParams;
use snowflake::Snowflake;
use sqlx::{types::uuid, PgPool};
use tokio::sync::RwLock;
use tracing::{debug, instrument};
use workspace_template::WorkspaceTemplateBuilder;

/// Verify the token from the gotrue server and create the user if it is a new user
/// Return true if the user is a new user
///
#[instrument(skip_all, err)]
pub async fn verify_token(
  pg_pool: &PgPool,
  id_gen: &Arc<RwLock<Snowflake>>,
  gotrue_client: &Client,
  access_token: &str,
) -> Result<bool, AppError> {
  let user = gotrue_client.user_info(access_token).await?;
  let user_uuid = uuid::Uuid::parse_str(&user.id)?;
  let name = name_from_user_metadata(&user.user_metadata);

  let mut txn = pg_pool
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
    let new_uid = id_gen.write().await.next_id();
    let workspace_id =
      create_user(txn.deref_mut(), new_uid, &user_uuid, &user.email, &name).await?;

    // Create the default workspace for the user. A default workspace might contain multiple
    // templates, e.g. a document template, a database template, etc.
    let templates = WorkspaceTemplateBuilder::new(new_uid, &workspace_id)
      .default_workspace()
      .await?;

    debug!("create {} templates for user:{}", templates.len(), new_uid);
    for template in templates {
      insert_into_af_collab(
        &mut txn,
        &new_uid,
        &InsertCollabParams {
          object_id: template.object_id,
          encoded_collab_v1: template
            .object_data
            .encode_to_bytes()
            .map_err(|err| AppError::Internal(anyhow::Error::from(err)))?,
          workspace_id: workspace_id.clone(),
          collab_type: template.object_type,
        },
      )
      .await?;
    }
  }
  txn
    .commit()
    .await
    .context("fail to commit transaction to verify token")?;
  Ok(is_new)
}

pub async fn get_profile(pg_pool: &PgPool, uuid: &Uuid) -> Result<AFUserProfile, AppError> {
  let row = select_user_profile(pg_pool, uuid)
    .await?
    .ok_or(AppError::RecordNotFound(format!(
      "Can't find the user profile for user: {}",
      uuid
    )))?;

  let profile = AFUserProfile::try_from(row)?;
  Ok(profile)
}

#[instrument(level = "debug", skip(pg_pool), err)]
pub async fn get_user_workspace_info(
  pg_pool: &PgPool,
  uuid: &Uuid,
) -> Result<AFUserWorkspaceInfo, AppError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("failed to acquire the transaction to query the user workspace info")?;
  let row = select_user_profile(txn.deref_mut(), uuid)
    .await?
    .ok_or(AppError::RecordNotFound(format!(
      "Can't find the user profile for {}",
      uuid
    )))?;

  // Get the latest workspace that the user has visited recently
  // TODO(nathan): the visiting_workspace might be None if the user get deleted from the workspace
  let visiting_workspace = AFWorkspace::try_from(
    select_workspace(txn.deref_mut(), &row.latest_workspace_id.unwrap()).await?,
  )?;

  // Get the user profile
  let user_profile = AFUserProfile::try_from(row)?;

  // Get all workspaces that the user can access to
  let workspaces = select_user_workspace(txn.deref_mut(), uuid)
    .await?
    .into_iter()
    .flat_map(|row| AFWorkspace::try_from(row).ok())
    .collect::<Vec<AFWorkspace>>();

  txn
    .commit()
    .await
    .context("failed to commit the transaction to get user workspace info")?;

  Ok(AFUserWorkspaceInfo {
    user_profile,
    visiting_workspace,
    workspaces,
  })
}

pub async fn update_user(
  pg_pool: &PgPool,
  user_uuid: Uuid,
  params: UpdateUserParams,
) -> Result<(), AppResponseError> {
  let metadata = params.metadata.map(|m| json!(m.into_inner()));
  Ok(database::user::update_user(pg_pool, &user_uuid, params.name, params.email, metadata).await?)
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

pub type UserListener = crate::biz::pg_listener::PostgresDBListener<AFUserNotification>;
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RealtimeUserImpl {
  pub uid: i64,
  pub device_id: String,
}

impl RealtimeUserImpl {
  pub fn new(uid: i64, device_id: String) -> Self {
    Self { uid, device_id }
  }
}

impl Display for RealtimeUserImpl {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "uid:{}|device_id:{}",
      self.uid, self.device_id,
    ))
  }
}

impl RealtimeUser for RealtimeUserImpl {
  fn uid(&self) -> i64 {
    self.uid
  }
}
