use crate::access_control::*;
use anyhow::anyhow;
use appflowy_cloud::biz;
use appflowy_cloud::biz::casbin::access_control::{Action, ObjectType};
use database_entity::dto::{AFAccessLevel, AFRole};
use serial_test::serial;
use shared_entity::dto::workspace_dto::CreateWorkspaceMember;
use sqlx::PgPool;

#[sqlx::test(migrations = false)]
#[serial]
async fn test_create_user(pool: PgPool) -> anyhow::Result<()> {
  let access_control = setup_access_control(&pool).await?;
  let user = create_user(&pool).await?;

  // Get workspace details
  let workspace = database::workspace::select_user_workspace(&pool, &user.uuid)
    .await?
    .into_iter()
    .next()
    .ok_or(anyhow!("workspace should be created"))?;

  assert!(access_control
    .enforce(
      &user.uid,
      &ObjectType::Workspace(&workspace.workspace_id.to_string()),
      AFRole::Owner
    )
    .await
    .context("user should be owner of its workspace")?);

  assert!(access_control
    .enforce(
      &user.uid,
      &ObjectType::Collab(&workspace.workspace_id.to_string()),
      AFAccessLevel::FullAccess,
    )
    .await
    .context("user should have full access of its collab")?);

  assert!(access_control
    .enforce(
      &user.uid,
      &ObjectType::Collab(&workspace.workspace_id.to_string()),
      Action::Read,
    )
    .await
    .context("user should be able to read its collab")?);

  assert!(access_control
    .enforce(
      &user.uid,
      &ObjectType::Collab(&workspace.workspace_id.to_string()),
      Action::Write,
    )
    .await
    .context("user should be able to write its collab")?);

  assert!(access_control
    .enforce(
      &user.uid,
      &ObjectType::Collab(&workspace.workspace_id.to_string()),
      Action::Delete,
    )
    .await
    .context("user should be able to delete its collab")?);

  Ok(())
}

#[sqlx::test(migrations = false)]
#[serial]
async fn test_add_users_to_workspace(pool: PgPool) -> anyhow::Result<()> {
  let access_control = setup_access_control(&pool).await?;
  let workspace_access_control = access_control.new_workspace_access_control();

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
  biz::workspace::ops::add_workspace_members(
    &pool,
    &user_main.uuid,
    &workspace.workspace_id,
    members,
    &workspace_access_control,
  )
  .await
  .context("adding users to workspace")?;
  {
    // Owner
    let user = user_owner;
    assert!(access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        AFAccessLevel::FullAccess,
      )
      .await
      .context("owner should have full access of its collab")?);
    assert!(access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        Action::Read,
      )
      .await
      .context("user should be able to read its collab")?);

    assert!(access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        Action::Write,
      )
      .await
      .context("user should be able to write its collab")?);

    assert!(access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        Action::Delete,
      )
      .await
      .context("user should be able to delete its collab")?);
  }

  {
    // Member
    let user = user_member;
    assert!(access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        AFAccessLevel::ReadAndWrite,
      )
      .await
      .context("member should have read write access of its collab")?);
    assert!(access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        Action::Read,
      )
      .await
      .context("user should be able to read its collab")?);

    assert!(access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        Action::Write,
      )
      .await
      .context("user should be able to write its collab")?);

    assert!(!access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        Action::Delete,
      )
      .await
      .context("user should not be able to delete its collab")?);
  }

  {
    // Guest
    let user = user_guest;
    assert!(access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        AFAccessLevel::ReadOnly,
      )
      .await
      .context("guest should have read only access of its collab")?);
    assert!(access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        Action::Read,
      )
      .await
      .context("user should not be able to read its collab")?);

    assert!(!access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        Action::Write,
      )
      .await
      .context("user should not be able to write its collab")?);

    assert!(!access_control
      .enforce(
        &user.uid,
        &ObjectType::Collab(&workspace.workspace_id.to_string()),
        Action::Delete,
      )
      .await
      .context("user should not be able to delete its collab")?);
  }

  Ok(())
}
