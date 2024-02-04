use crate::casbin::{
  assert_workspace_role, assert_workspace_role_error, create_user, setup_db, MODEL_CONF,
};
use anyhow::{anyhow, Context};
use app_error::ErrorCode;
use appflowy_cloud::biz;
use appflowy_cloud::biz::casbin::access_control::CasbinAccessControl;
use appflowy_cloud::biz::casbin::adapter::PgAdapter;
use appflowy_cloud::biz::pg_listener::PgListeners;
use casbin::{CoreApi, DefaultModel, Enforcer};
use database_entity::dto::AFRole;
use shared_entity::dto::workspace_dto::{CreateWorkspaceMember, WorkspaceMemberChangeset};
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn test_workspace_access_control_get_role(pool: PgPool) -> anyhow::Result<()> {
  setup_db(&pool).await?;

  let model = DefaultModel::from_str(MODEL_CONF).await?;
  let enforcer = Enforcer::new(model, PgAdapter::new(pool.clone())).await?;
  let listeners = PgListeners::new(&pool).await?;
  let access_control = CasbinAccessControl::new(
    pool.clone(),
    listeners.subscribe_collab_member_change(),
    listeners.subscribe_workspace_member_change(),
    enforcer,
  );
  let access_control = access_control.new_workspace_access_control();

  let user = create_user(&pool).await?;

  // Get workspace details
  let workspace = database::workspace::select_user_workspace(&pool, &user.uuid)
    .await?
    .into_iter()
    .next()
    .ok_or(anyhow!("workspace should be created"))?;

  assert_workspace_role(
    &access_control,
    &user.uid,
    &workspace.workspace_id,
    Some(AFRole::Owner),
  )
  .await;

  let member = create_user(&pool).await?;
  let _ = biz::workspace::ops::add_workspace_members(
    &pool,
    &member.uuid,
    &workspace.workspace_id,
    vec![CreateWorkspaceMember {
      email: member.email.clone(),
      role: AFRole::Member,
    }],
  )
  .await
  .context("adding users to workspace")?;

  assert_workspace_role(
    &access_control,
    &member.uid,
    &workspace.workspace_id,
    Some(AFRole::Member),
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
    &access_control,
    &member.uid,
    &workspace.workspace_id,
    Some(AFRole::Guest),
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
    &access_control,
    &member.uid,
    &workspace.workspace_id,
    ErrorCode::NotEnoughPermissions,
  )
  .await;

  Ok(())
}
