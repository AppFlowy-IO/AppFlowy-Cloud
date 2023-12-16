use std::{str::FromStr, sync::Arc};

use actix_web::http::Method;
use anyhow::anyhow;
use async_trait::async_trait;
use casbin::{CoreApi, MgmtApi};
use database::user::select_uid_from_uuid;
use sqlx::PgPool;
use tokio::sync::{broadcast, RwLock};
use tracing::log::warn;
use tracing::{event, instrument};
use uuid::Uuid;

use crate::biz::{
  collab::member_listener::{CollabMemberAction, CollabMemberNotification},
  workspace::{
    access_control::WorkspaceAccessControl,
    member_listener::{WorkspaceMemberAction, WorkspaceMemberNotification},
  },
};
use app_error::AppError;
use database_entity::dto::{AFAccessLevel, AFCollabMember, AFRole};

use realtime::collaborate::{CollabAccessControl, CollabUserId};

use super::{
  Action, ActionType, ObjectType, POLICY_FIELD_INDEX_ACTION, POLICY_FIELD_INDEX_OBJECT,
  POLICY_FIELD_INDEX_USER,
};

/// Manages access control using Casbin.
///
/// Stores access control policies in the form `subject, object, role`
/// where `subject` is `uid`, `object` is `oid`, and `role` is [AFAccessLevel] or [AFRole].
///
/// Roles are mapped to the corresponding actions that they are allowed to perform.
/// `FullAccess` has write
/// `FullAccess` has read
///
/// Access control requests are made in the form `subject, object, action`
/// and will be evaluated against the policies and mappings stored,
/// according to the model defined.
pub struct CasbinAccessControl {
  pg_pool: PgPool,
  enforcer: Arc<RwLock<casbin::Enforcer>>,
}

impl Clone for CasbinAccessControl {
  fn clone(&self) -> Self {
    Self {
      pg_pool: self.pg_pool.clone(),
      enforcer: Arc::clone(&self.enforcer),
    }
  }
}

impl CasbinAccessControl {
  pub fn new(
    pg_pool: PgPool,
    collab_listener: broadcast::Receiver<CollabMemberNotification>,
    workspace_listener: broadcast::Receiver<WorkspaceMemberNotification>,
    enforcer: casbin::Enforcer,
  ) -> Self {
    let enforcer = Arc::new(RwLock::new(enforcer));
    spawn_listen_on_workspace_member_change(workspace_listener, enforcer.clone());
    spawn_listen_on_collab_member_change(collab_listener, enforcer.clone());
    Self { pg_pool, enforcer }
  }
  pub fn new_collab_access_control(&self) -> CasbinCollabAccessControl {
    CasbinCollabAccessControl {
      casbin_access_control: self.clone(),
    }
  }

  pub fn new_workspace_access_control(&self) -> CasbinWorkspaceAccessControl {
    CasbinWorkspaceAccessControl {
      casbin_access_control: self.clone(),
    }
  }

  /// Update permission for a user.
  ///
  /// [`ObjectType::Workspace`] has to be paired with [`ActionType::Role`],
  /// [`ObjectType::Collab`] has to be paired with [`ActionType::Level`],
  #[instrument(level = "trace", skip(self, obj, act), err)]
  async fn update(
    &self,
    uid: &i64,
    obj: &ObjectType<'_>,
    act: &ActionType,
  ) -> Result<bool, AppError> {
    let (obj_id, action) = match (obj, act) {
      (ObjectType::Workspace(_), ActionType::Role(role)) => {
        Ok((obj.to_string(), i32::from(role.clone()).to_string()))
      },
      (ObjectType::Collab(_), ActionType::Level(level)) => {
        Ok((obj.to_string(), i32::from(*level).to_string()))
      },
      _ => Err(AppError::Internal(anyhow!(
        "invalid object type and action type combination: object={:?}, action={:?}",
        obj,
        act
      ))),
    }?;

    let mut enforcer = self.enforcer.write().await;

    let current_policy = enforcer
      .get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![obj_id.clone()])
      .into_iter()
      .find(|p| p[POLICY_FIELD_INDEX_USER] == uid.to_string());

    if let Some(current_policy) = current_policy {
      enforcer
        .remove_policy(current_policy)
        .await
        .map_err(|e| AppError::Internal(anyhow!("casbin error removing policy: {e:?}")))?;
    }

    event!(
      tracing::Level::TRACE,
      "updating policy: object={}, user={},action={}",
      obj_id,
      uid,
      action
    );
    enforcer
      .add_policy(vec![uid.to_string(), obj_id, action])
      .await
      .map_err(|e| AppError::Internal(anyhow!("casbin error adding policy: {e:?}")))
  }

  async fn remove(&self, uid: &i64, obj: &ObjectType<'_>) -> Result<bool, AppError> {
    let obj_id = obj.to_string();
    let mut enforcer = self.enforcer.write().await;

    let policies = enforcer.get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![obj_id]);

    let rem = policies
      .into_iter()
      .filter(|p| p[POLICY_FIELD_INDEX_USER] == uid.to_string())
      .collect();

    enforcer
      .remove_policies(rem)
      .await
      .map_err(|e| AppError::Internal(anyhow!("casbin error enforce: {e:?}")))
  }

  /// Get uid which is used for all operations.
  async fn get_uid(&self, user: &CollabUserId<'_>) -> Result<i64, AppError> {
    let uid = match user {
      CollabUserId::UserId(uid) => **uid,
      CollabUserId::UserUuid(uuid) => select_uid_from_uuid(&self.pg_pool, uuid).await?,
    };
    Ok(uid)
  }

  async fn get_collab_member(&self, uid: &i64, oid: &str) -> Result<AFCollabMember, AppError> {
    database::collab::select_collab_member(uid, oid, &self.pg_pool).await
  }

  async fn get_workspace_member_role(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
  ) -> Result<AFRole, AppError> {
    database::workspace::select_workspace_member(&self.pg_pool, uid, workspace_id)
      .await
      .map(|r| r.role)
  }
}

fn spawn_listen_on_collab_member_change(
  mut listener: broadcast::Receiver<CollabMemberNotification>,
  enforcer: Arc<RwLock<casbin::Enforcer>>,
) {
  tokio::spawn(async move {
    while let Ok(change) = listener.recv().await {
      match change.action_type {
        CollabMemberAction::INSERT | CollabMemberAction::UPDATE => {
          if let (Some(_), Some(_)) = (change.new_oid(), change.new_uid()) {
            if let Err(err) = enforcer.write().await.load_policy().await {
              warn!(
                "Failed to reload the collab member status from db: {:?}, error: {}",
                change, err
              );
            }
          } else {
            warn!("The oid or uid is None")
          }
        },
        CollabMemberAction::DELETE => {
          if let (Some(_), Some(_)) = (change.old_oid(), change.old_uid()) {
            if let Err(err) = enforcer.write().await.load_policy().await {
              warn!(
                "Failed to reload the collab member status from db: {:?}, error: {}",
                change, err
              );
            }
          } else {
            warn!("The oid or uid is None")
          }
        },
      }
    }
  });
}

fn spawn_listen_on_workspace_member_change(
  mut listener: broadcast::Receiver<WorkspaceMemberNotification>,
  enforcer: Arc<RwLock<casbin::Enforcer>>,
) {
  tokio::spawn(async move {
    while let Ok(change) = listener.recv().await {
      match change.action_type {
        WorkspaceMemberAction::INSERT | WorkspaceMemberAction::UPDATE => match change.new {
          None => {
            warn!("The workspace member change can't be None when the action is INSERT or UPDATE")
          },
          Some(_) => {
            if let Err(err) = enforcer.write().await.load_policy().await {
              warn!("Failed to reload workspace member status from db: {}", err);
            }
          },
        },
        WorkspaceMemberAction::DELETE => match change.old {
          None => warn!("The workspace member change can't be None when the action is DELETE"),
          Some(_) => {
            if let Err(err) = enforcer.write().await.load_policy().await {
              warn!("Failed to reload workspace member status from db: {}", err);
            }
          },
        },
      }
    }
  });
}

#[derive(Clone)]
pub struct CasbinCollabAccessControl {
  casbin_access_control: CasbinAccessControl,
}

impl CasbinCollabAccessControl {
  pub async fn update_member(&self, uid: &i64, oid: &str, access_level: AFAccessLevel) {
    let _ = self
      .casbin_access_control
      .update(
        uid,
        &ObjectType::Collab(oid),
        &ActionType::Level(access_level),
      )
      .await;
  }
  pub async fn remove_member(&self, uid: &i64, oid: &str) {
    let _ = self
      .casbin_access_control
      .remove(uid, &ObjectType::Collab(oid))
      .await;
  }
}

#[async_trait]
impl CollabAccessControl for CasbinCollabAccessControl {
  async fn get_collab_access_level(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
  ) -> Result<AFAccessLevel, AppError> {
    let uid = self.casbin_access_control.get_uid(&user).await?;
    let collab_id = ObjectType::Collab(oid).to_string();
    let policies = self
      .casbin_access_control
      .enforcer
      .read()
      .await
      .get_filtered_policy(POLICY_FIELD_INDEX_OBJECT, vec![collab_id]);

    // There should only be one entry per user per object, which is enforced in [CasbinAccessControl], so just take one using next.
    let mut access_level = policies
      .into_iter()
      .find(|p| p[POLICY_FIELD_INDEX_USER] == uid.to_string())
      .map(|p| p[POLICY_FIELD_INDEX_ACTION].clone())
      .and_then(|s| i32::from_str(s.as_str()).ok())
      .map(AFAccessLevel::from);

    if access_level.is_none() {
      if let Ok(member) = self
        .casbin_access_control
        .get_collab_member(&uid, oid)
        .await
      {
        access_level = Some(member.permission.access_level);
        self
          .casbin_access_control
          .update(
            &uid,
            &ObjectType::Collab(oid),
            &ActionType::Level(member.permission.access_level),
          )
          .await?;
      }
    }

    access_level.ok_or(AppError::RecordNotFound(format!(
      "collab:{} does not exist or user:{} is not a member",
      uid, oid
    )))
  }

  async fn cache_collab_access_level(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
    level: AFAccessLevel,
  ) -> Result<(), AppError> {
    let uid = self.casbin_access_control.get_uid(&user).await?;
    self
      .casbin_access_control
      .update(&uid, &ObjectType::Collab(oid), &ActionType::Level(level))
      .await?;

    Ok(())
  }

  async fn can_access_http_method(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
    method: &Method,
  ) -> Result<bool, AppError> {
    let uid = self.casbin_access_control.get_uid(&user).await?;
    let action = if Method::POST == method || Method::PUT == method || Method::DELETE == method {
      Action::Write
    } else {
      Action::Read
    };

    // If collab does not exist, allow access.
    // Workspace access control will still check it.
    let collab_exists = self
      .casbin_access_control
      .enforcer
      .read()
      .await
      .get_all_objects()
      .contains(&ObjectType::Collab(oid).to_string());

    if !collab_exists {
      return Ok(true);
    }

    self
      .casbin_access_control
      .enforcer
      .read()
      .await
      .enforce((
        uid.to_string(),
        ObjectType::Collab(oid).to_string(),
        action.to_string(),
      ))
      .map_err(|e| AppError::Internal(anyhow!("casbin error enforce: {e:?}")))
  }

  async fn can_send_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    self
      .casbin_access_control
      .enforcer
      .read()
      .await
      .enforce((
        uid.to_string(),
        ObjectType::Collab(oid).to_string(),
        Action::Write.to_string(),
      ))
      .map_err(|e| AppError::Internal(anyhow!("casbin error enforce: {e:?}")))
  }

  async fn can_receive_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    self
      .casbin_access_control
      .enforcer
      .read()
      .await
      .enforce((
        uid.to_string(),
        ObjectType::Collab(oid).to_string(),
        Action::Read.to_string(),
      ))
      .map_err(|e| AppError::Internal(anyhow!("casbin error enforce: {e:?}")))
  }
}

#[derive(Clone)]
pub struct CasbinWorkspaceAccessControl {
  casbin_access_control: CasbinAccessControl,
}

#[async_trait]
impl WorkspaceAccessControl for CasbinWorkspaceAccessControl {
  async fn get_role_from_uuid(
    &self,
    user_uuid: &Uuid,
    workspace_id: &Uuid,
  ) -> Result<AFRole, AppError> {
    let uid = self
      .casbin_access_control
      .get_uid(&CollabUserId::UserUuid(user_uuid))
      .await?;
    self.get_role_from_uid(&uid, workspace_id).await
  }
  async fn get_role_from_uid(&self, uid: &i64, workspace_id: &Uuid) -> Result<AFRole, AppError> {
    let policies_future = self
      .casbin_access_control
      .enforcer
      .read()
      .await
      .get_filtered_policy(
        POLICY_FIELD_INDEX_OBJECT,
        vec![ObjectType::Workspace(&workspace_id.to_string()).to_string()],
      );

    let role = match policies_future
      .into_iter()
      .find(|p| p[POLICY_FIELD_INDEX_USER] == uid.to_string())
    {
      Some(policy) => i32::from_str(policy[POLICY_FIELD_INDEX_ACTION].as_str())
        .ok()
        .map(AFRole::from),
      None => self
        .casbin_access_control
        .get_workspace_member_role(uid, workspace_id)
        .await
        .ok(),
    };

    role.ok_or_else(|| {
      AppError::NotEnoughPermissions(format!(
        "user:{} is not a member of workspace:{}",
        uid, workspace_id
      ))
    })
  }

  async fn update_member(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    role: AFRole,
  ) -> Result<(), AppError> {
    let _ = self
      .casbin_access_control
      .update(
        uid,
        &ObjectType::Workspace(&workspace_id.to_string()),
        &ActionType::Role(role),
      )
      .await?;
    Ok(())
  }

  async fn remove_member(&self, uid: &i64, workspace_id: &Uuid) -> Result<(), AppError> {
    let _ = self
      .casbin_access_control
      .remove(uid, &ObjectType::Workspace(&workspace_id.to_string()))
      .await?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use anyhow::Context;
  use casbin::{DefaultModel, Enforcer};
  use shared_entity::dto::workspace_dto::{CreateWorkspaceMember, WorkspaceMemberChangeset};

  use crate::biz::{
    self,
    casbin::{adapter::PgAdapter, tests},
    pg_listener::PgListeners,
  };

  use super::*;

  #[sqlx::test(migrations = false)]
  async fn test_workspace_access_control_get_role(pool: PgPool) -> anyhow::Result<()> {
    tests::setup_db(&pool).await?;

    let model = DefaultModel::from_str(tests::MODEL_CONF).await?;
    let enforcer = Enforcer::new(model, PgAdapter::new(pool.clone())).await?;
    let listeners = PgListeners::new(&pool).await?;
    let access_control = CasbinAccessControl::new(
      pool.clone(),
      listeners.subscribe_collab_member_change(),
      listeners.subscribe_workspace_member_change(),
      enforcer,
    );
    let access_control = access_control.new_workspace_access_control();

    let user = tests::create_user(&pool).await?;

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

    let member = tests::create_user(&pool).await?;
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

  #[sqlx::test(migrations = false)]
  async fn test_collab_access_control_get_access_level(pool: PgPool) -> anyhow::Result<()> {
    tests::setup_db(&pool).await?;

    let model = DefaultModel::from_str(tests::MODEL_CONF).await?;
    let enforcer = Enforcer::new(model, PgAdapter::new(pool.clone())).await?;
    let listeners = PgListeners::new(&pool).await?;
    let access_control = CasbinAccessControl::new(
      pool.clone(),
      listeners.subscribe_collab_member_change(),
      listeners.subscribe_workspace_member_change(),
      enforcer,
    );
    let access_control = access_control.new_collab_access_control();

    let user = tests::create_user(&pool).await?;
    let owner = tests::create_user(&pool).await?;
    let member = tests::create_user(&pool).await?;
    let guest = tests::create_user(&pool).await?;

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
    let _ = biz::workspace::ops::add_workspace_members(
      &pool,
      &user.uuid,
      &workspace.workspace_id,
      members,
    )
    .await
    .context("adding users to workspace")?;

    assert_eq!(
      AFAccessLevel::FullAccess,
      access_control
        .get_collab_access_level(
          CollabUserId::UserUuid(&user.uuid),
          &workspace.workspace_id.to_string(),
        )
        .await?
    );
    assert_eq!(
      AFAccessLevel::FullAccess,
      access_control
        .get_collab_access_level(
          CollabUserId::UserId(&user.uid),
          &workspace.workspace_id.to_string(),
        )
        .await?
    );

    assert_eq!(
      AFAccessLevel::FullAccess,
      access_control
        .get_collab_access_level(
          CollabUserId::UserId(&owner.uid),
          &workspace.workspace_id.to_string(),
        )
        .await?
    );

    assert_eq!(
      AFAccessLevel::ReadAndWrite,
      access_control
        .get_collab_access_level(
          CollabUserId::UserId(&member.uid),
          &workspace.workspace_id.to_string(),
        )
        .await?
    );

    assert_eq!(
      AFAccessLevel::ReadOnly,
      access_control
        .get_collab_access_level(
          CollabUserId::UserId(&guest.uid),
          &workspace.workspace_id.to_string(),
        )
        .await?
    );

    // wait for update message
    let mut collab_listener = listeners.subscribe_collab_member_change();

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

    let _ = collab_listener.recv().await;

    assert_eq!(
      AFAccessLevel::ReadAndComment,
      access_control
        .get_collab_access_level(
          CollabUserId::UserId(&guest.uid),
          &workspace.workspace_id.to_string(),
        )
        .await?
    );

    // wait for delete message
    let mut collab_listener = listeners.subscribe_collab_member_change();

    database::collab::delete_collab_member(guest.uid, &workspace.workspace_id.to_string(), &pool)
      .await
      .context("delete collab member")?;

    let _ = collab_listener.recv().await;

    assert!(access_control
      .get_collab_access_level(
        CollabUserId::UserId(&guest.uid),
        &workspace.workspace_id.to_string()
      )
      .await
      .expect_err("user should not be part of collab")
      .is_record_not_found());

    Ok(())
  }

  #[sqlx::test(migrations = false)]
  async fn test_collab_access_control_access_http_method(pool: PgPool) -> anyhow::Result<()> {
    tests::setup_db(&pool).await?;

    let model = DefaultModel::from_str(tests::MODEL_CONF).await?;
    let enforcer = Enforcer::new(model, PgAdapter::new(pool.clone())).await?;
    let listeners = PgListeners::new(&pool).await?;
    let access_control = CasbinAccessControl::new(
      pool.clone(),
      listeners.subscribe_collab_member_change(),
      listeners.subscribe_workspace_member_change(),
      enforcer,
    );
    let access_control = access_control.new_collab_access_control();

    let user = tests::create_user(&pool).await?;
    let guest = tests::create_user(&pool).await?;
    let stranger = tests::create_user(&pool).await?;

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

    assert!(
      access_control
        .can_access_http_method(
          CollabUserId::UserId(&user.uid),
          &workspace.workspace_id.to_string(),
          &Method::GET
        )
        .await?
    );

    assert!(
      access_control
        .can_access_http_method(
          CollabUserId::UserId(&user.uid),
          &workspace.workspace_id.to_string(),
          &Method::POST
        )
        .await?
    );

    assert!(
      access_control
        .can_access_http_method(
          CollabUserId::UserId(&user.uid),
          &workspace.workspace_id.to_string(),
          &Method::PUT
        )
        .await?
    );

    assert!(
      access_control
        .can_access_http_method(
          CollabUserId::UserId(&user.uid),
          &workspace.workspace_id.to_string(),
          &Method::DELETE
        )
        .await?
    );

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

    assert!(
      access_control
        .can_access_http_method(
          CollabUserId::UserId(&guest.uid),
          &workspace.workspace_id.to_string(),
          &Method::GET
        )
        .await?,
      "guest should have read access"
    );

    assert!(
      !access_control
        .can_access_http_method(
          CollabUserId::UserId(&guest.uid),
          &workspace.workspace_id.to_string(),
          &Method::POST
        )
        .await?,
      "guest should not have write access"
    );

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
  async fn test_collab_access_control_send_receive_collab_update(
    pool: PgPool,
  ) -> anyhow::Result<()> {
    tests::setup_db(&pool).await?;

    let model = DefaultModel::from_str(tests::MODEL_CONF).await?;
    let enforcer = Enforcer::new(model, PgAdapter::new(pool.clone())).await?;
    let listeners = PgListeners::new(&pool).await?;
    let access_control = CasbinAccessControl::new(
      pool.clone(),
      listeners.subscribe_collab_member_change(),
      listeners.subscribe_workspace_member_change(),
      enforcer,
    );
    let access_control = access_control.new_collab_access_control();

    let user = tests::create_user(&pool).await?;
    let guest = tests::create_user(&pool).await?;
    let stranger = tests::create_user(&pool).await?;

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
  async fn test_collab_access_control_cache_collab_access_level(
    pool: PgPool,
  ) -> anyhow::Result<()> {
    tests::setup_db(&pool).await?;

    let model = DefaultModel::from_str(tests::MODEL_CONF).await?;
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
    tests::setup_db(&pool).await?;

    let model = DefaultModel::from_str(tests::MODEL_CONF).await?;
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

    assert!(access_control.enforcer.read().await.enforce((
      uid.to_string(),
      ObjectType::Workspace("123").to_string(),
      i32::from(AFRole::Owner).to_string(),
    ))?);

    assert!(
      access_control
        .remove(&uid, &ObjectType::Workspace("123"))
        .await?
    );

    assert!(!access_control.enforcer.read().await.enforce((
      uid.to_string(),
      ObjectType::Workspace("123").to_string(),
      i32::from(AFRole::Owner).to_string(),
    ))?);

    Ok(())
  }
}
