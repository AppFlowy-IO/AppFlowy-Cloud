use crate::biz::collab::member_listener::{CollabMemberAction, CollabMemberChange};
use crate::biz::collab::ops::require_user_can_edit;
use crate::component::auth::jwt::UserUuid;
use crate::middleware::access_control_mw::{AccessControlService, AccessResource};
use anyhow::Error;
use async_trait::async_trait;
use database_entity::{AFCollabMember, AFPermissionLevel, AFRole};
use realtime::collaborate::CollabPermission;
use shared_entity::app_error::AppError;
use sqlx::PgPool;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::trace;
use uuid::Uuid;

#[derive(Clone)]
pub struct CollabAccessControl;

#[async_trait]
impl AccessControlService for CollabAccessControl {
  fn resource(&self) -> AccessResource {
    AccessResource::Collab
  }

  async fn check_collab_permission(
    &self,
    workspace_id: &Uuid,
    oid: &str,
    user_uuid: &UserUuid,
    pg_pool: &PgPool,
  ) -> Result<(), AppError> {
    trace!(
      "Collab access control: oid: {:?}, user_uuid: {:?}",
      oid,
      user_uuid
    );
    require_user_can_edit(pg_pool, workspace_id, user_uuid, oid).await
  }
}

type CollabMemberStatusByOid = HashMap<String, MemberStatus>;

/// Use to check if the user is allowed to send or receive the [CollabMessage]
pub struct CollabPermissionImpl {
  pg_pool: PgPool,
  role_by_uid: Arc<RwLock<HashMap<i64, CollabMemberStatusByOid>>>,
}

#[derive(Clone, Debug)]
enum MemberStatus {
  Deleted,
  Valid(AFPermissionLevel),
}

impl CollabPermissionImpl {
  pub fn new(pg_pool: PgPool, mut listener: broadcast::Receiver<CollabMemberChange>) -> Self {
    let role_by_uid = Arc::new(RwLock::new(HashMap::new()));

    // Update the role of the user when the role of the collab member is changed
    let cloned_role_by_uid = role_by_uid.clone();
    tokio::spawn(async move {
      while let Ok(change) = listener.recv().await {
        match change.action_type {
          CollabMemberAction::Insert | CollabMemberAction::Update => {
            let mut outer_map = cloned_role_by_uid.write().await;
            let inner_map = outer_map.entry(change.uid).or_insert_with(HashMap::new);
            inner_map.insert(change.oid.clone(), MemberStatus::Valid(change.role));
          },
          CollabMemberAction::Delete => {
            if let Some(mut inner_map) = cloned_role_by_uid.write().await.get_mut(&change.uid) {
              inner_map.insert(change.oid.clone(), MemberStatus::Deleted);
            }
          },
        }
      }
    });

    Self {
      pg_pool,
      role_by_uid,
    }
  }

  /// Return the role of the user in the collab
  async fn get_role_state(&self, uid: i64, oid: &str) -> Option<MemberStatus> {
    self
      .role_by_uid
      .read()
      .await
      .get(&uid)
      .map(|map| map.get(oid).cloned())?
  }

  #[inline]
  async fn refresh_state_from_db(&self, uid: i64, oid: &str) -> Result<MemberStatus, Error> {
    let member = database::collab::select_collab_member(uid, oid, &self.pg_pool).await?;
    let mut outer_map = self.role_by_uid.write().await;
    let inner_map = outer_map.entry(uid).or_insert_with(HashMap::new);
    inner_map.insert(
      member.oid,
      MemberStatus::Valid(member.permission.access_level.clone()),
    );
    Ok(MemberStatus::Valid(member.permission.access_level))
  }

  #[inline]
  async fn is_user_can_edit_collab(&self, uid: i64, oid: &str) -> Result<bool, Error> {
    let role_status = match self.get_role_state(uid, oid).await {
      None => self.refresh_state_from_db(uid, oid).await?,
      Some(status) => status,
    };

    match role_status {
      MemberStatus::Deleted => Ok(false),
      MemberStatus::Valid(access_level) => match access_level {
        AFPermissionLevel::ReadOnly | AFPermissionLevel::ReadAndComment => Ok(false),
        AFPermissionLevel::ReadAndWrite | AFPermissionLevel::FullAccess => Ok(true),
      },
    }
  }
}

#[async_trait]
impl CollabPermission for CollabPermissionImpl {
  #[inline]
  async fn is_allowed_send_by_user(&self, uid: i64, oid: &str) -> Result<bool, Error> {
    self.is_user_can_edit_collab(uid, oid).await
  }

  #[inline]
  async fn is_allowed_recv_by_user(&self, uid: i64, oid: &str) -> Result<bool, Error> {
    self.is_user_can_edit_collab(uid, oid).await
  }
}
