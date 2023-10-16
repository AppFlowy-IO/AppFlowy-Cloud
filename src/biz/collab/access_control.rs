use crate::biz::collab::member_listener::{CollabMemberAction, CollabMemberChange};
use crate::middleware::access_control_mw::{AccessResource, HttpAccessControlService};

use async_trait::async_trait;
use database_entity::AFAccessLevel;
use realtime::collaborate::{CollabPermission, CollabUserId};
use shared_entity::app_error::AppError;
use shared_entity::error_code::ErrorCode;
use sqlx::PgPool;

use actix_web::http::Method;
use database::user::select_uid_from_uuid;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::trace;
use uuid::Uuid;

#[derive(Clone)]
pub struct CollabAccessControl<P: CollabPermission>(pub Arc<P>);

#[async_trait]
impl<P> HttpAccessControlService for CollabAccessControl<P>
where
  P: CollabPermission,
  AppError: From<<P as CollabPermission>::Error>,
{
  fn resource(&self) -> AccessResource {
    AccessResource::Collab
  }

  async fn check_collab_permission(
    &self,
    _workspace_id: &Uuid,
    oid: &str,
    user_uuid: &Uuid,
    _pg_pool: &PgPool,
    method: Method,
  ) -> Result<(), AppError> {
    trace!(
      "Collab access control: oid: {:?}, user_uuid: {:?}",
      oid,
      user_uuid
    );

    match self
      .0
      .can_access_http_method(CollabUserId::UserUuid(user_uuid), oid, method)
      .await
      .map_err(AppError::from)
    {
      Ok(can_access) => {
        if can_access {
          Ok(())
        } else {
          Err(AppError::new(ErrorCode::NotEnoughPermissions, "123"))
        }
      },
      Err(err) => {
        // If the collab is not found which means the user is the owner of the collab
        if err.is_record_not_found() {
          Ok(())
        } else {
          Err(err)
        }
      },
    }
  }
}

type MemberStatusByUid = HashMap<i64, CollabMemberStatusByOid>;
type CollabMemberStatusByOid = HashMap<String, MemberStatus>;

/// Use to check if the user is allowed to send or receive the [CollabMessage]
pub struct CollabPermissionImpl {
  pg_pool: PgPool,
  member_status_by_uid: Arc<RwLock<MemberStatusByUid>>,
}

#[derive(Clone, Debug)]
enum MemberStatus {
  Deleted,
  Valid(AFAccessLevel),
}

impl CollabPermissionImpl {
  pub fn new(pg_pool: PgPool, mut listener: broadcast::Receiver<CollabMemberChange>) -> Self {
    let member_status_by_uid = Arc::new(RwLock::new(HashMap::new()));

    // Update the role of the user when the role of the collab member is changed
    let cloned_pg_pool = pg_pool.clone();
    let cloned_member_status_by_uid = member_status_by_uid.clone();
    tokio::spawn(async move {
      while let Ok(change) = listener.recv().await {
        let oid = change.oid().to_string();
        let uid = change.uid();
        match change.action_type {
          CollabMemberAction::INSERT | CollabMemberAction::UPDATE => {
            let _ = refresh_from_db(uid, &oid, &cloned_pg_pool, &cloned_member_status_by_uid).await;
          },
          CollabMemberAction::DELETE => {
            if let Some(inner_map) = cloned_member_status_by_uid.write().await.get_mut(&uid) {
              inner_map.insert(oid, MemberStatus::Deleted);
            }
          },
        }
      }
    });

    Self {
      pg_pool,
      member_status_by_uid,
    }
  }

  /// Return the role of the user in the collab
  async fn get_role_state(&self, uid: i64, oid: &str) -> Option<MemberStatus> {
    self
      .member_status_by_uid
      .read()
      .await
      .get(&uid)
      .map(|map| map.get(oid).cloned())?
  }

  #[inline]
  async fn get_user_collab_access_level(
    &self,
    uid: i64,
    oid: &str,
  ) -> Result<Option<AFAccessLevel>, AppError> {
    let role_status = match self.get_role_state(uid, oid).await {
      None => refresh_from_db(uid, oid, &self.pg_pool, &self.member_status_by_uid).await?,
      Some(status) => status,
    };

    match role_status {
      MemberStatus::Deleted => Ok(None),
      MemberStatus::Valid(access_level) => Ok(Some(access_level)),
    }
  }
}

#[inline]
async fn refresh_from_db(
  uid: i64,
  oid: &str,
  pg_pool: &PgPool,
  member_status_by_uid: &Arc<RwLock<MemberStatusByUid>>,
) -> Result<MemberStatus, AppError> {
  let member = database::collab::select_collab_member_permission(uid, oid, pg_pool).await?;
  let mut outer_map = member_status_by_uid.write().await;
  let inner_map = outer_map.entry(uid).or_insert_with(HashMap::new);
  inner_map.insert(
    member.oid,
    MemberStatus::Valid(member.permission.access_level.clone()),
  );
  Ok(MemberStatus::Valid(member.permission.access_level))
}
#[async_trait]
impl CollabPermission for CollabPermissionImpl {
  type Error = AppError;

  async fn can_access_http_method(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
    method: Method,
  ) -> Result<bool, Self::Error> {
    let level = match user {
      CollabUserId::UserId(uid) => self.get_user_collab_access_level(*uid, oid).await,
      CollabUserId::UserUuid(uuid) => {
        let uid = select_uid_from_uuid(&self.pg_pool, uuid).await?;
        self.get_user_collab_access_level(uid, oid).await
      },
    }?;

    trace!("Collab member access level: {:?}", level);
    let can = match level {
      None => false,
      Some(level) => {
        if Method::POST == method || Method::PUT == method || Method::DELETE == method {
          level.can_write()
        } else {
          true
        }
      },
    };

    Ok(can)
  }

  #[inline]
  async fn can_send_message(&self, uid: i64, oid: &str) -> Result<bool, Self::Error> {
    match self.get_user_collab_access_level(uid, oid).await? {
      None => Ok(false),
      Some(level) => match level {
        AFAccessLevel::ReadOnly | AFAccessLevel::ReadAndComment => Ok(false),
        AFAccessLevel::ReadAndWrite | AFAccessLevel::FullAccess => Ok(true),
      },
    }
  }

  #[inline]
  async fn can_receive_message(&self, uid: i64, oid: &str) -> Result<bool, Self::Error> {
    match self.get_user_collab_access_level(uid, oid).await? {
      None => Ok(false),
      Some(_) => Ok(true),
    }
  }
}
