use crate::casbin::{create_user, setup_db, MODEL_CONF};
use anyhow::{anyhow, Context};
use appflowy_cloud::biz;
use appflowy_cloud::biz::casbin::access_control::CasbinAccessControl;
use appflowy_cloud::biz::casbin::adapter::PgAdapter;
use appflowy_cloud::biz::pg_listener::PgListeners;
use appflowy_cloud::biz::workspace::access_control::WorkspaceAccessControl;
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

  assert_eq!(
    AFRole::Owner,
    access_control
      .get_role_from_uuid(&user.uuid, &workspace.workspace_id)
      .await?
  );

  assert_eq!(
    AFRole::Owner,
    access_control
      .get_role_from_uid(&user.uid, &workspace.workspace_id)
      .await?
  );

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

  assert_eq!(
    AFRole::Member,
    access_control
      .get_role_from_uuid(&member.uuid, &workspace.workspace_id)
      .await?
  );

  assert_eq!(
    AFRole::Member,
    access_control
      .get_role_from_uid(&member.uid, &workspace.workspace_id)
      .await?
  );

  // wait for update message
  let mut workspace_listener = listeners.subscribe_workspace_member_change();

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

  let _ = workspace_listener.recv().await;

  assert_eq!(
    AFRole::Guest,
    access_control
      .get_role_from_uid(&member.uid, &workspace.workspace_id)
      .await?
  );

  // wait for delete message
  let mut workspace_listener = listeners.subscribe_workspace_member_change();

  biz::workspace::ops::remove_workspace_members(
    &user.uuid,
    &pool,
    &workspace.workspace_id,
    &[member.email.clone()],
  )
  .await
  .context("removing users from workspace")?;

  let _ = workspace_listener.recv().await;

  assert!(access_control
    .get_role_from_uid(&member.uid, &workspace.workspace_id)
    .await
    .expect_err("user should not be part of workspace")
    .is_not_enough_permissions());

  Ok(())
}
