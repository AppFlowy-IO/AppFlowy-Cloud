use crate::access_control::{
  assert_workspace_role, assert_workspace_role_error, create_user, setup_access_control,
};
use anyhow::{anyhow, Context};
use app_error::ErrorCode;
use appflowy_cloud::biz;
use serial_test::serial;

use database_entity::dto::AFRole;
use shared_entity::dto::workspace_dto::{CreateWorkspaceMember, WorkspaceMemberChangeset};
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
#[serial]
async fn test_workspace_access_control_get_role(pool: PgPool) -> anyhow::Result<()> {
  let access_control = setup_access_control(&pool).await?;
  let workspace_access_control = access_control.new_workspace_access_control();

  let user = create_user(&pool).await?;

  // Get workspace details
  let workspace = database::workspace::select_user_workspace(&pool, &user.uuid)
    .await?
    .into_iter()
    .next()
    .ok_or(anyhow!("workspace should be created"))?;

  assert_workspace_role(
    &workspace_access_control,
    &user.uid,
    &workspace.workspace_id,
    Some(AFRole::Owner),
    &pool,
  )
  .await;

  let member = create_user(&pool).await?;
  biz::workspace::ops::add_workspace_members(
    &pool,
    &member.uuid,
    &workspace.workspace_id,
    vec![CreateWorkspaceMember {
      email: member.email.clone(),
      role: AFRole::Member,
    }],
    &workspace_access_control,
  )
  .await
  .context("adding users to workspace")?;

  assert_workspace_role(
    &workspace_access_control,
    &member.uid,
    &workspace.workspace_id,
    Some(AFRole::Member),
    &pool,
  )
  .await;

  // wait for update message
  biz::workspace::ops::update_workspace_member(
    &pool,
    &workspace.workspace_id,
    &WorkspaceMemberChangeset {
      email: member.email.clone(),
      role: Some(AFRole::Guest),
      name: None,
    },
  )
  .await
  .context("update user workspace role")?;

  assert_workspace_role(
    &workspace_access_control,
    &member.uid,
    &workspace.workspace_id,
    Some(AFRole::Guest),
    &pool,
  )
  .await;

  biz::workspace::ops::remove_workspace_members(
    &user.uuid,
    &pool,
    &workspace.workspace_id,
    &[member.email.clone()],
  )
  .await
  .context("removing users from workspace")?;

  assert_workspace_role_error(
    &workspace_access_control,
    &member.uid,
    &workspace.workspace_id,
    ErrorCode::RecordNotFound,
    &pool,
  )
  .await;

  Ok(())
}
