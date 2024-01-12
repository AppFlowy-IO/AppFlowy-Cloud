use crate::casbin::*;
use actix_http::Method;
use anyhow::{anyhow, Context};
use appflowy_cloud::biz;
use appflowy_cloud::biz::casbin::access_control::CasbinAccessControl;
use appflowy_cloud::biz::casbin::adapter::PgAdapter;
use appflowy_cloud::biz::casbin::{ActionType, ObjectType};
use appflowy_cloud::biz::pg_listener::PgListeners;
use casbin::{CoreApi, DefaultModel, Enforcer};
use database_entity::dto::{AFAccessLevel, AFRole};
use realtime::collaborate::{CollabAccessControl, CollabUserId};
use shared_entity::dto::workspace_dto::CreateWorkspaceMember;
use sqlx::PgPool;
use std::time::Duration;
use tokio::time::sleep;

#[sqlx::test(migrations = false)]
async fn test_collab_access_control_get_access_level(pool: PgPool) -> anyhow::Result<()> {
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
  let access_control = access_control.new_collab_access_control();

  let user = create_user(&pool).await?;
  let owner = create_user(&pool).await?;
  let member = create_user(&pool).await?;
  let guest = create_user(&pool).await?;

  // Get workspace details
  let workspace = database::workspace::select_user_workspace(&pool, &user.uuid)
    .await?
    .into_iter()
    .next()
    .ok_or(anyhow!("workspace should be created"))?;

  let members = vec![
    CreateWorkspaceMember {
      email: owner.email.clone(),
      role: AFRole::Owner,
    },
    CreateWorkspaceMember {
      email: member.email.clone(),
      role: AFRole::Member,
    },
    CreateWorkspaceMember {
      email: guest.email.clone(),
      role: AFRole::Guest,
    },
  ];
  let _ =
    biz::workspace::ops::add_workspace_members(&pool, &user.uuid, &workspace.workspace_id, members)
      .await
      .context("adding users to workspace")?;

  assert_access_level(
    &access_control,
    &user.uid,
    workspace.workspace_id.to_string(),
    Some(AFAccessLevel::FullAccess),
  )
  .await;

  assert_access_level(
    &access_control,
    &member.uid,
    workspace.workspace_id.to_string(),
    Some(AFAccessLevel::ReadAndWrite),
  )
  .await;

  assert_access_level(
    &access_control,
    &guest.uid,
    workspace.workspace_id.to_string(),
    Some(AFAccessLevel::ReadOnly),
  )
  .await;

  let mut txn = pool
    .begin()
    .await
    .context("acquire transaction to update collab member")?;

  database::collab::upsert_collab_member_with_txn(
    guest.uid,
    &workspace.workspace_id.to_string(),
    &AFAccessLevel::ReadAndComment,
    &mut txn,
  )
  .await?;

  txn
    .commit()
    .await
    .expect("commit transaction to update collab member");

  assert_access_level(
    &access_control,
    &guest.uid,
    workspace.workspace_id.to_string(),
    Some(AFAccessLevel::ReadAndComment),
  )
  .await;

  database::collab::delete_collab_member(guest.uid, &workspace.workspace_id.to_string(), &pool)
    .await
    .context("delete collab member")?;

  assert_access_level(
    &access_control,
    &guest.uid,
    workspace.workspace_id.to_string(),
    None,
  )
  .await;
  Ok(())
}

#[sqlx::test(migrations = false)]
async fn test_collab_access_control_access_http_method(pool: PgPool) -> anyhow::Result<()> {
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
  let access_control = access_control.new_collab_access_control();

  let user = create_user(&pool).await?;
  let guest = create_user(&pool).await?;
  let stranger = create_user(&pool).await?;

  // Get workspace details
  let workspace = database::workspace::select_user_workspace(&pool, &user.uuid)
    .await?
    .into_iter()
    .next()
    .ok_or(anyhow!("workspace should be created"))?;

  let _ = biz::workspace::ops::add_workspace_members(
    &pool,
    &guest.uuid,
    &workspace.workspace_id,
    vec![CreateWorkspaceMember {
      email: guest.email,
      role: AFRole::Guest,
    }],
  )
  .await
  .context("adding users to workspace")?;

  for method in [Method::GET, Method::POST, Method::PUT, Method::DELETE] {
    assert_can_access_http_method(
      &access_control,
      &user.uid,
      &workspace.workspace_id.to_string(),
      method,
      true,
    )
    .await;
  }

  assert!(
    access_control
      .can_access_http_method(
        CollabUserId::UserId(&user.uid),
        "new collab oid",
        &Method::POST
      )
      .await?,
    "should have access to non-existent collab oid"
  );

  // guest should have read access
  assert_can_access_http_method(
    &access_control,
    &guest.uid,
    &workspace.workspace_id.to_string(),
    Method::GET,
    true,
  )
  .await;

  // guest should not have write access
  assert_can_access_http_method(
    &access_control,
    &guest.uid,
    &workspace.workspace_id.to_string(),
    Method::POST,
    false,
  )
  .await;

  assert!(
    !access_control
      .can_access_http_method(
        CollabUserId::UserId(&stranger.uid),
        &workspace.workspace_id.to_string(),
        &Method::GET
      )
      .await?,
    "stranger should not have read access"
  );

  assert!(
    !access_control
      .can_access_http_method(
        CollabUserId::UserId(&stranger.uid),
        &workspace.workspace_id.to_string(),
        &Method::POST
      )
      .await?,
    "stranger should not have write access"
  );

  Ok(())
}

#[sqlx::test(migrations = false)]
async fn test_collab_access_control_send_receive_collab_update(pool: PgPool) -> anyhow::Result<()> {
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
  let access_control = access_control.new_collab_access_control();

  let user = create_user(&pool).await?;
  let guest = create_user(&pool).await?;
  let stranger = create_user(&pool).await?;

  // Get workspace details
  let workspace = database::workspace::select_user_workspace(&pool, &user.uuid)
    .await?
    .into_iter()
    .next()
    .ok_or(anyhow!("workspace should be created"))?;

  let _ = biz::workspace::ops::add_workspace_members(
    &pool,
    &guest.uuid,
    &workspace.workspace_id,
    vec![CreateWorkspaceMember {
      email: guest.email,
      role: AFRole::Guest,
    }],
  )
  .await
  .context("adding users to workspace")?;

  // Need to wait for the listener(spawn_listen_on_workspace_member_change) to receive the event
  //
  sleep(Duration::from_secs(1)).await;

  assert!(
    access_control
      .can_send_collab_update(&user.uid, &workspace.workspace_id.to_string())
      .await?
  );

  assert!(
    access_control
      .can_receive_collab_update(&user.uid, &workspace.workspace_id.to_string())
      .await?
  );

  assert!(
    !access_control
      .can_send_collab_update(&guest.uid, &workspace.workspace_id.to_string())
      .await?,
    "guest cannot send collab update"
  );

  assert!(
    access_control
      .can_receive_collab_update(&guest.uid, &workspace.workspace_id.to_string())
      .await?,
    "guest can receive collab update"
  );

  assert!(
    !access_control
      .can_send_collab_update(&stranger.uid, &workspace.workspace_id.to_string())
      .await?,
    "stranger cannot send collab update"
  );

  assert!(
    !access_control
      .can_receive_collab_update(&stranger.uid, &workspace.workspace_id.to_string())
      .await?,
    "stranger cannot receive collab update"
  );

  Ok(())
}

#[sqlx::test(migrations = false)]
async fn test_collab_access_control_cache_collab_access_level(pool: PgPool) -> anyhow::Result<()> {
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
  let access_control = access_control.new_collab_access_control();

  let uid = 123;
  let oid = "collab::oid".to_owned();
  access_control
    .cache_collab_access_level(CollabUserId::UserId(&uid), &oid, AFAccessLevel::FullAccess)
    .await?;

  assert_eq!(
    AFAccessLevel::FullAccess,
    access_control
      .get_collab_access_level(CollabUserId::UserId(&uid), &oid)
      .await?
  );

  access_control
    .cache_collab_access_level(CollabUserId::UserId(&uid), &oid, AFAccessLevel::ReadOnly)
    .await?;

  assert_eq!(
    AFAccessLevel::ReadOnly,
    access_control
      .get_collab_access_level(CollabUserId::UserId(&uid), &oid)
      .await?
  );

  Ok(())
}

#[sqlx::test(migrations = false)]
async fn test_casbin_access_control_update_remove(pool: PgPool) -> anyhow::Result<()> {
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

  let uid = 123;
  assert!(
    access_control
      .update(
        &uid,
        &ObjectType::Workspace("123"),
        &ActionType::Role(AFRole::Owner)
      )
      .await?
  );
  assert!(access_control.get_enforcer().read().await.enforce((
    uid.to_string(),
    ObjectType::Workspace("123").to_string(),
    i32::from(AFRole::Owner).to_string(),
  ))?);

  assert!(
    access_control
      .remove(&uid, &ObjectType::Workspace("123"))
      .await?
  );

  assert!(!access_control.get_enforcer().read().await.enforce((
    uid.to_string(),
    ObjectType::Workspace("123").to_string(),
    i32::from(AFRole::Owner).to_string(),
  ))?);

  Ok(())
}
