use std::{str::FromStr, sync::Arc};

use actix_web::http::Method;
use anyhow::anyhow;
use async_trait::async_trait;
use casbin::{CoreApi, MgmtApi};
use database::user::select_uid_from_uuid;
use sqlx::PgPool;
use tokio::sync::{broadcast, RwLock};
use tracing::instrument;
use tracing::log::warn;

use uuid::Uuid;

use crate::biz::{
  collab::member_listener::{CollabMemberAction, CollabMemberNotification},
  workspace::{
    access_control::WorkspaceAccessControl,
    member_listener::{WorkspaceMemberAction, WorkspaceMemberNotification},
  },
};
use app_error::AppError;
use database::workspace::select_permission;
use database_entity::dto::{AFAccessLevel, AFCollabMember, AFRole};

use crate::biz::casbin::enforcer_ext::{enforcer_remove, enforcer_update};
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
    spawn_listen_on_collab_member_change(pg_pool.clone(), collab_listener, enforcer.clone());
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

  /// Only expose this method for testing
  #[cfg(debug_assertions)]
  pub fn get_enforcer(&self) -> &Arc<RwLock<casbin::Enforcer>> {
    &self.enforcer
  }

  pub async fn update(
    &self,
    uid: &i64,
    obj: &ObjectType<'_>,
    act: &ActionType,
  ) -> Result<bool, AppError> {
    enforcer_update(&self.enforcer, uid, obj, act).await
  }

  pub async fn remove(&self, uid: &i64, obj: &ObjectType<'_>) -> Result<bool, AppError> {
    let mut enforcer = self.enforcer.write().await;
    enforcer_remove(&mut enforcer, uid, obj).await
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
  pg_pool: PgPool,
  mut listener: broadcast::Receiver<CollabMemberNotification>,
  enforcer: Arc<RwLock<casbin::Enforcer>>,
) {
  tokio::spawn(async move {
    while let Ok(change) = listener.recv().await {
      match change.action_type {
        CollabMemberAction::INSERT | CollabMemberAction::UPDATE => {
          if let Some(member_row) = change.new {
            if let Ok(Some(row)) = select_permission(&pg_pool, &member_row.permission_id).await {
              if let Err(err) = enforcer_update(
                &enforcer,
                &member_row.uid,
                &ObjectType::Collab(&member_row.oid),
                &ActionType::Level(row.access_level),
              )
              .await
              {
                warn!(
                  "Failed to update the user:{} collab{} access control, error: {}",
                  member_row.uid, member_row.oid, err
                );
              }
            }
          } else {
            warn!("The new collab member is None")
          }
        },
        CollabMemberAction::DELETE => {
          if let (Some(oid), Some(uid)) = (change.old_oid(), change.old_uid()) {
            let mut enforcer = enforcer.write().await;
            if let Err(err) = enforcer_remove(&mut enforcer, uid, &ObjectType::Collab(oid)).await {
              warn!(
                "Failed to remove the user:{} collab{} access control, error: {}",
                uid, oid, err
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
          Some(member_row) => {
            if let Err(err) = enforcer_update(
              &enforcer,
              &member_row.uid,
              &ObjectType::Workspace(&member_row.workspace_id.to_string()),
              &ActionType::Role(AFRole::from(member_row.role_id)),
            )
            .await
            {
              warn!(
                "Failed to update the user:{} workspace:{} access control, error: {}",
                member_row.uid, member_row.workspace_id, err
              );
            }
          },
        },
        WorkspaceMemberAction::DELETE => match change.old {
          None => warn!("The workspace member change can't be None when the action is DELETE"),
          Some(member_row) => {
            let mut enforcer = enforcer.write().await;
            if let Err(err) = enforcer_remove(
              &mut enforcer,
              &member_row.uid,
              &ObjectType::Workspace(&member_row.workspace_id.to_string()),
            )
            .await
            {
              warn!(
                "Failed to remove the user:{} workspace: {} access control, error: {}",
                member_row.uid, member_row.workspace_id, err
              );
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
  #[instrument(level = "trace", skip_all)]
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
      oid, uid
    )))
  }

  #[instrument(level = "trace", skip_all)]
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
    let policies = self
      .casbin_access_control
      .enforcer
      .read()
      .await
      .get_filtered_policy(
        POLICY_FIELD_INDEX_OBJECT,
        vec![ObjectType::Workspace(&workspace_id.to_string()).to_string()],
      );

    let role = match policies
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

  #[instrument(level = "trace", skip_all)]
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
