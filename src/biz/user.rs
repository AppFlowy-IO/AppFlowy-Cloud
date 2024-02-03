use anyhow::{Context, Result};
use gotrue::api::Client;

use realtime::collaborate::CollabAccessControl;
use serde_json::json;
use shared_entity::response::AppResponseError;
use std::fmt::{Display, Formatter};
use std::ops::DerefMut;
use std::sync::Arc;
use uuid::Uuid;

use database::workspace::{select_user_profile, select_user_workspace, select_workspace};
use database_entity::dto::{
  AFAccessLevel, AFRole, AFUserProfile, AFUserWorkspaceInfo, AFWorkspace, CollabParams,
};

use crate::biz::workspace::access_control::WorkspaceAccessControl;
use app_error::AppError;
use database::collab::insert_into_af_collab;
use database::pg_row::AFUserNotification;
use database::user::{create_user, is_user_exist};
use realtime::entities::RealtimeUser;
use shared_entity::dto::auth_dto::UpdateUserParams;
use snowflake::Snowflake;
use sqlx::{types::uuid, PgPool, Transaction};
use tokio::sync::RwLock;
use tracing::{debug, event, instrument};
use workspace_template::document::get_started::GetStartedDocumentTemplate;
use workspace_template::{WorkspaceTemplate, WorkspaceTemplateBuilder};

/// Verify the token from the gotrue server and create the user if it is a new user
/// Return true if the user is a new user
///
#[instrument(skip_all, err)]
pub async fn verify_token<W, C>(
  pg_pool: &PgPool,
  id_gen: &Arc<RwLock<Snowflake>>,
  gotrue_client: &Client,
  access_token: &str,
  workspace_access_control: &W,
  collab_access_control: &C,
) -> Result<bool, AppError>
where
  W: WorkspaceAccessControl,
  C: CollabAccessControl,
{
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
    event!(tracing::Level::INFO, "create new user:{}", new_uid);
    let workspace_id =
      create_user(txn.deref_mut(), new_uid, &user_uuid, &user.email, &name).await?;

    workspace_access_control
      .update_member(
        &new_uid,
        &Uuid::parse_str(&workspace_id).unwrap(),
        AFRole::Owner,
      )
      .await?;

    collab_access_control
      .cache_collab_access_level(&new_uid, &workspace_id, AFAccessLevel::FullAccess)
      .await?;

    // Create a workspace with the GetStarted template
    create_workspace_for_user(
      new_uid,
      &workspace_id,
      collab_access_control,
      &mut txn,
      vec![GetStartedDocumentTemplate],
    )
    .await?;
  }
  txn
    .commit()
    .await
    .context("fail to commit transaction to verify token")?;
  Ok(is_new)
}

/// Create a workspace for a user.
/// This function generates a workspace along with its templates and stores them in the database.
/// Each template is stored as an individual collaborative object.
async fn create_workspace_for_user<C, T>(
  new_uid: i64,
  workspace_id: &str,
  collab_access_control: &C,
  txn: &mut Transaction<'_, sqlx::Postgres>,
  templates: Vec<T>,
) -> Result<(), AppError>
where
  C: CollabAccessControl,
  T: WorkspaceTemplate + Send + Sync + 'static,
{
  let templates = WorkspaceTemplateBuilder::new(new_uid, workspace_id)
    .with_templates(templates)
    .build()
    .await?;

  debug!("create {} templates for user:{}", templates.len(), new_uid);
  for template in templates {
    let object_id = template.object_id;
    let encoded_collab_v1 = template
      .object_data
      .encode_to_bytes()
      .map_err(|err| AppError::Internal(anyhow::Error::from(err)))?;

    collab_access_control
      .cache_collab_access_level(&new_uid, &object_id, AFAccessLevel::FullAccess)
      .await?;

    insert_into_af_collab(
      txn,
      &new_uid,
      workspace_id,
      &CollabParams {
        object_id,
        encoded_collab_v1,
        collab_type: template.object_type,
        override_if_exist: false,
      },
    )
    .await?;
  }
  Ok(())
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
  /// Use the unique connect ID to identify users. This is important because a user with the same device ID
  /// may disconnect and then reconnect. In such cases, the server could receive the connect signal before
  /// the disconnect signal. The unique connect ID helps in correctly identifying and managing the user's
  /// session under these circumstances.
  pub connect_id: String,
}

impl RealtimeUserImpl {
  pub fn new(uid: i64, device_id: String) -> Self {
    Self {
      uid,
      device_id,
      connect_id: Uuid::new_v4().to_string(),
    }
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
