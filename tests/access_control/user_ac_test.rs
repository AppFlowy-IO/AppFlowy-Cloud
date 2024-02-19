use crate::access_control::*;
use anyhow::anyhow;
use appflowy_cloud::biz;
use appflowy_cloud::biz::casbin::access_control::{Action, ObjectType, ToCasbinAction, MODEL_CONF};
use appflowy_cloud::biz::casbin::adapter::PgAdapter;
use casbin::{CoreApi, DefaultModel, Enforcer};
use database_entity::dto::{AFAccessLevel, AFRole};
use shared_entity::dto::workspace_dto::CreateWorkspaceMember;
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
async fn test_create_user(pool: PgPool) -> anyhow::Result<()> {
  setup_db(&pool).await?;

  let user = create_user(&pool).await?;

  // Get workspace details
  let workspace = database::workspace::select_user_workspace(&pool, &user.uuid)
    .await?
    .into_iter()
    .next()
    .ok_or(anyhow!("workspace should be created"))?;

  let model = DefaultModel::from_str(MODEL_CONF).await?;
  let enforcer = Enforcer::new(model, PgAdapter::new(pool.clone())).await?;

  assert!(enforcer
    .enforce((
      user.uid.to_string(),
      ObjectType::Workspace(&workspace.workspace_id.to_string()).to_object_id(),
      AFRole::Owner.to_action()
    ))
    .context("user should be owner of its workspace")?);

  assert!(enforcer
    .enforce((
      user.uid.to_string(),
      ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
      AFAccessLevel::FullAccess.to_action(),
    ))
    .context("user should have full access of its collab")?);

  assert!(enforcer
    .enforce((
      user.uid.to_string(),
      ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
      Action::Read.to_action(),
    ))
    .context("user should be able to read its collab")?);

  assert!(enforcer
    .enforce((
      user.uid.to_string(),
      ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
      Action::Write.to_action(),
    ))
    .context("user should be able to write its collab")?);

  assert!(enforcer
    .enforce((
      user.uid.to_string(),
      ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
      Action::Delete.to_action(),
    ))
    .context("user should be able to delete its collab")?);

  Ok(())
}

#[sqlx::test(migrations = false)]
async fn test_add_users_to_workspace(pool: PgPool) -> anyhow::Result<()> {
  setup_db(&pool).await?;

  let user_main = create_user(&pool).await?;
  let user_owner = create_user(&pool).await?;
  let user_member = create_user(&pool).await?;
  let user_guest = create_user(&pool).await?;

  // Get workspace details
  let workspace = database::workspace::select_user_workspace(&pool, &user_main.uuid)
    .await?
    .into_iter()
    .next()
    .ok_or(anyhow!("workspace should be created"))?;

  let members = vec![
    CreateWorkspaceMember {
      email: user_owner.email.clone(),
      role: AFRole::Owner,
    },
    CreateWorkspaceMember {
      email: user_member.email.clone(),
      role: AFRole::Member,
    },
    CreateWorkspaceMember {
      email: user_guest.email.clone(),
      role: AFRole::Guest,
    },
  ];
  let _ = biz::workspace::ops::add_workspace_members(
    &pool,
    &user_main.uuid,
    &workspace.workspace_id,
    members,
  )
  .await
  .context("adding users to workspace")?;

  let model = DefaultModel::from_str(MODEL_CONF).await?;
  let enforcer = Enforcer::new(model, PgAdapter::new(pool.clone())).await?;

  {
    // Owner
    let user = user_owner;
    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        AFAccessLevel::FullAccess.to_action(),
      ))
      .context("owner should have full access of its collab")?);
    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        Action::Read.to_action(),
      ))
      .context("user should be able to read its collab")?);

    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        Action::Write.to_action(),
      ))
      .context("user should be able to write its collab")?);

    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        Action::Delete.to_action(),
      ))
      .context("user should be able to delete its collab")?);
  }

  {
    // Member
    let user = user_member;
    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        AFAccessLevel::ReadAndWrite.to_action(),
      ))
      .context("member should have read write access of its collab")?);
    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        Action::Read.to_action(),
      ))
      .context("user should be able to read its collab")?);

    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        Action::Write.to_action(),
      ))
      .context("user should be able to write its collab")?);

    assert!(!enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        Action::Delete.to_action(),
      ))
      .context("user should not be able to delete its collab")?);
  }

  {
    // Guest
    let user = user_guest;
    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        AFAccessLevel::ReadOnly.to_action(),
      ))
      .context("guest should have read only access of its collab")?);
    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        Action::Read.to_action(),
      ))
      .context("user should not be able to read its collab")?);

    assert!(!enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        Action::Write.to_action(),
      ))
      .context("user should not be able to write its collab")?);

    assert!(!enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
        Action::Delete.to_action(),
      ))
      .context("user should not be able to delete its collab")?);
  }

  Ok(())
}

#[sqlx::test(migrations = false)]
async fn test_reload_policy_after_adding_user_to_workspace(pool: PgPool) -> anyhow::Result<()> {
  setup_db(&pool).await?;

  let user_owner = create_user(&pool).await?;
  let user_member = create_user(&pool).await?;

  // Get workspace details
  let workspace = database::workspace::select_user_workspace(&pool, &user_owner.uuid)
    .await?
    .into_iter()
    .next()
    .ok_or(anyhow!("workspace should be created"))?;

  // Create enforcer before adding user to workspace
  let model = DefaultModel::from_str(MODEL_CONF).await?;
  let mut enforcer = Enforcer::new(model, PgAdapter::new(pool.clone())).await?;

  let members = vec![CreateWorkspaceMember {
    email: user_member.email.clone(),
    role: AFRole::Member,
  }];
  let _ = biz::workspace::ops::add_workspace_members(
    &pool,
    &user_owner.uuid,
    &workspace.workspace_id,
    members,
  )
  .await
  .context("adding users to workspace")?;

  assert!(!enforcer
    .enforce((
      user_member.uid.to_string(),
      ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
      AFAccessLevel::ReadAndWrite.to_action(),
    ))
    .context("member should not have read write access to collab before reload")?);

  enforcer.load_policy().await?;

  assert!(enforcer
    .enforce((
      user_member.uid.to_string(),
      ObjectType::Collab(&workspace.workspace_id.to_string()).to_object_id(),
      AFAccessLevel::ReadAndWrite.to_action(),
    ))
    .context("member should have read write access to collab")?);

  Ok(())
}
