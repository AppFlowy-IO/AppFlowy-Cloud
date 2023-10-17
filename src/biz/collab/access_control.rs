use crate::biz::collab::member_listener::{CollabMemberAction, CollabMemberChange};
use crate::middleware::access_control_mw::{AccessResource, HttpAccessControlService};

use async_trait::async_trait;
use database_entity::AFAccessLevel;
use realtime::collaborate::{CollabPermission, CollabUserId};
use shared_entity::app_error::AppError;
use shared_entity::error_code::ErrorCode;
use sqlx::PgPool;

use actix_router::{Path, Url};
use actix_web::http::Method;
use database::user::select_uid_from_uuid;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{trace, warn};
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
    _path: Path<Url>,
  ) -> Result<(), AppError> {
    trace!("oid: {:?}, user_uuid: {:?}", oid, user_uuid);

    let can_access = self
      .0
      .can_access_http_method(CollabUserId::UserUuid(user_uuid), oid, &method)
      .await
      .map_err(AppError::from)?;

    if !can_access {
      return Err(AppError::new(
        ErrorCode::NotEnoughPermissions,
        format!(
          "Not enough permissions to access the collab: {} with http method: {}",
          oid, method
        ),
      ));
    }
    Ok(())
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
        match change.action_type {
          CollabMemberAction::INSERT | CollabMemberAction::UPDATE => {
            if let (Some(oid), Some(uid)) = (change.new_oid(), change.new_uid()) {
              let _ =
                refresh_from_db(uid, oid, &cloned_pg_pool, &cloned_member_status_by_uid).await;
            } else {
              warn!("The oid or uid is None")
            }
          },
          CollabMemberAction::DELETE => {
            if let (Some(oid), Some(uid)) = (change.old_oid(), change.old_uid()) {
              if let Some(inner_map) = cloned_member_status_by_uid.write().await.get_mut(uid) {
                inner_map.insert(oid.to_string(), MemberStatus::Deleted);
              }
            } else {
              warn!("The oid or uid is None")
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
  async fn get_role_state(&self, uid: &i64, oid: &str) -> Option<MemberStatus> {
    self
      .member_status_by_uid
      .read()
      .await
      .get(uid)
      .map(|map| map.get(oid).cloned())?
  }

  #[inline]
  async fn get_user_collab_access_level(
    &self,
    uid: &i64,
    oid: &str,
  ) -> Result<AFAccessLevel, AppError> {
    let member_status = match self.get_role_state(uid, oid).await {
      None => refresh_from_db(uid, oid, &self.pg_pool, &self.member_status_by_uid).await?,
      Some(status) => status,
    };

    match member_status {
      MemberStatus::Deleted => Err(AppError::new(
        ErrorCode::NotEnoughPermissions,
        "The user is not the member of the collab",
      )),
      MemberStatus::Valid(access_level) => Ok(access_level),
    }
  }
}

#[inline]
async fn refresh_from_db(
  uid: &i64,
  oid: &str,
  pg_pool: &PgPool,
  member_status_by_uid: &Arc<RwLock<MemberStatusByUid>>,
) -> Result<MemberStatus, AppError> {
  let member = database::collab::select_collab_member(uid, oid, pg_pool).await?;
  let mut outer_map = member_status_by_uid.write().await;
  let inner_map = outer_map.entry(*uid).or_insert_with(HashMap::new);
  inner_map.insert(
    member.oid,
    MemberStatus::Valid(member.permission.access_level.clone()),
  );
  Ok(MemberStatus::Valid(member.permission.access_level))
}
#[async_trait]
impl CollabPermission for CollabPermissionImpl {
  type Error = AppError;

  async fn get_access_level(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
  ) -> Result<Option<AFAccessLevel>, Self::Error> {
    let result = match user {
      CollabUserId::UserId(uid) => self.get_user_collab_access_level(uid, oid).await,
      CollabUserId::UserUuid(uuid) => {
        let uid = select_uid_from_uuid(&self.pg_pool, uuid).await?;
        self.get_user_collab_access_level(&uid, oid).await
      },
    };

    match result {
      Ok(level) => Ok(Some(level)),
      Err(err) => {
        if err.is_record_not_found() {
          Ok(None)
        } else {
          Err(err)
        }
      },
    }
  }

  async fn can_access_http_method(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
    method: &Method,
  ) -> Result<bool, Self::Error> {
    let level = self.get_access_level(user, oid).await?;
    trace!("access level: {:?}", level);

    match level {
      None => Ok(true),
      Some(level) => {
        if Method::POST == method || Method::PUT == method || Method::DELETE == method {
          Ok(level.can_write())
        } else {
          Ok(true)
        }
      },
    }
  }

  #[inline]
  async fn can_send_message(&self, uid: &i64, oid: &str) -> Result<bool, Self::Error> {
    match self
      .get_access_level(CollabUserId::UserId(uid), oid)
      .await?
    {
      None => Ok(true),
      Some(level) => match level {
        AFAccessLevel::ReadOnly | AFAccessLevel::ReadAndComment => Ok(false),
        AFAccessLevel::ReadAndWrite | AFAccessLevel::FullAccess => Ok(true),
      },
    }
  }

  #[inline]
  async fn can_receive_message(&self, uid: &i64, oid: &str) -> Result<bool, Self::Error> {
    Ok(
      self
        .get_access_level(CollabUserId::UserId(uid), oid)
        .await
        .is_ok(),
    )
  }
}
