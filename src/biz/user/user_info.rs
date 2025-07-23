use app_error::AppError;
use database::workspace::{
  select_all_user_non_guest_workspaces, select_all_user_workspaces, select_user_profile,
  select_workspace_with_count_and_role,
};
use database_entity::dto::{AFUserProfile, AFUserWorkspaceInfo, AFWorkspace};
use serde_json::json;
use shared_entity::dto::auth_dto::UpdateUserParams;
use shared_entity::response::AppResponseError;
use sqlx::PgPool;
use tracing::instrument;
use uuid::Uuid;

pub async fn get_profile(pg_pool: &PgPool, uuid: &Uuid) -> anyhow::Result<AFUserProfile, AppError> {
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
  exclude_guest: bool,
) -> anyhow::Result<AFUserWorkspaceInfo, AppError> {
  let row = select_user_profile(pg_pool, uuid)
    .await?
    .ok_or(AppError::RecordNotFound(format!(
      "Can't find the user profile for {}",
      uuid
    )))?;

  let latest_workspace_id = row.latest_workspace_id;

  // Get the user profile
  let user_profile = AFUserProfile::try_from(row)?;

  // Get all workspaces that the user can access to
  let workspaces_rows = if exclude_guest {
    select_all_user_non_guest_workspaces(pg_pool, uuid).await?
  } else {
    select_all_user_workspaces(pg_pool, uuid).await?
  };
  let workspaces = workspaces_rows
    .into_iter()
    .flat_map(|row| AFWorkspace::try_from(row).ok())
    .collect::<Vec<AFWorkspace>>();

  if workspaces.is_empty() {
    return Err(AppError::RecordNotFound(format!(
      "Can't find any workspace for user: {}",
      uuid
    )));
  }

  // safety: safe to unwrap since workspaces is not empty
  let first_workspace = workspaces.first().cloned().unwrap();

  let visiting_workspace = match latest_workspace_id {
    Some(workspace_id) => AFWorkspace::try_from(
      select_workspace_with_count_and_role(pg_pool, &workspace_id, user_profile.uid).await?,
    )?,
    None => first_workspace,
  };

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
) -> anyhow::Result<(), AppResponseError> {
  let metadata = params.metadata.map(|m| json!(m.into_inner()));
  Ok(database::user::update_user(pg_pool, &user_uuid, params.name, params.email, metadata).await?)
}
