use crate::biz::collab::member_listener::{CollabMemberAction, CollabMemberChange};
use crate::biz::workspace::access_control::WorkspaceAccessControl;
use crate::middleware::access_control_mw::{AccessResource, HttpAccessControlService};
use actix_router::{Path, Url};
use actix_web::http::Method;
use async_trait::async_trait;
use database::collab::CollabStorageAccessControl;
use database::user::select_uid_from_uuid;
use database_entity::database_error::DatabaseError;
use database_entity::{AFAccessLevel, AFRole};
use realtime::collaborate::{CollabAccessControl, CollabUserId};
use shared_entity::app_error::AppError;
use shared_entity::error_code::ErrorCode;
use sqlx::PgPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{instrument, trace, warn};
use uuid::Uuid;

/// Represents the access levels of various collaboration objects for a user.
/// - Key: User's UID.
/// - Value: A mapping between the collaboration object's OID and the user's role within that collaboration.
type MemberStatusByUid = HashMap<i64, CollabMemberStatusByOid>;

/// Represents the access level of a collaboration object identified by its OID.
/// - Key: OID of the collaboration object.
/// - Value: The user's role within the collaboration.
type CollabMemberStatusByOid = HashMap<String, MemberStatus>;

/// Used to cache the access level of a user for collaboration objects.
/// The cache will be updated after the user's access level for a collaboration object is changed.
/// The change is broadcasted by the `CollabMemberListener` or set by the [CollabAccessControlImpl::update_member] method.
///
/// TODO(nathan): broadcast the member access level changes to all connected devices
///
pub struct CollabAccessControlImpl {
  pg_pool: PgPool,
  member_status_by_uid: Arc<RwLock<MemberStatusByUid>>,
}

#[derive(Clone, Debug)]
enum MemberStatus {
  /// The user is not the member of the collab. When the user is not the member of the collab,
  /// it don't need to query the database to get the role of the user in the collab
  Deleted,
  /// The user is the member of the collab
  Valid(AFAccessLevel),
}

impl CollabAccessControlImpl {
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

  /// The member's access level may be altered by PostgreSQL notifications. However, there are instances
  /// where these notifications aren't received promptly, leading to potential inconsistencies in the user's access level.
  /// Therefore, it's essential to update the user's access level in the cache whenever there's a change.
  pub async fn update_member(&self, uid: &i64, oid: &str, access_level: AFAccessLevel) {
    update_member_status(uid, oid, access_level, &self.member_status_by_uid).await;
  }

  pub async fn remove_member(&self, uid: &i64, oid: &str) {
    if let Some(inner_map) = self.member_status_by_uid.write().await.get_mut(uid) {
      inner_map.insert(oid.to_string(), MemberStatus::Deleted);
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
async fn update_member_status(
  uid: &i64,
  oid: &str,
  access_level: AFAccessLevel,
  member_status_by_uid: &Arc<RwLock<MemberStatusByUid>>,
) {
  let mut outer_map = member_status_by_uid.write().await;
  let inner_map = outer_map.entry(*uid).or_insert_with(HashMap::new);
  inner_map.insert(oid.to_string(), MemberStatus::Valid(access_level));
}

#[inline]
async fn refresh_from_db(
  uid: &i64,
  oid: &str,
  pg_pool: &PgPool,
  member_status_by_uid: &Arc<RwLock<MemberStatusByUid>>,
) -> Result<MemberStatus, AppError> {
  let member = database::collab::select_collab_member(uid, oid, pg_pool).await?;
  let status = MemberStatus::Valid(member.permission.access_level.clone());
  update_member_status(
    uid,
    oid,
    member.permission.access_level,
    member_status_by_uid,
  )
  .await;
  Ok(status)
}

#[async_trait]
impl CollabAccessControl for CollabAccessControlImpl {
  type Error = AppError;
  async fn get_collab_access_level(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
  ) -> Result<AFAccessLevel, Self::Error> {
    let level = match user {
      CollabUserId::UserId(uid) => self.get_user_collab_access_level(uid, oid).await,
      CollabUserId::UserUuid(uuid) => {
        let uid = select_uid_from_uuid(&self.pg_pool, uuid).await?;
        self.get_user_collab_access_level(&uid, oid).await
      },
    }?;

    Ok(level)
  }

  async fn can_access_http_method(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
    method: &Method,
  ) -> Result<bool, Self::Error> {
    match self.get_collab_access_level(user, oid).await {
      Ok(level) => {
        if Method::POST == method || Method::PUT == method || Method::DELETE == method {
          Ok(level.can_write())
        } else {
          Ok(true)
        }
      },
      Err(err) => {
        if err.is_record_not_found() {
          Ok(true)
        } else {
          Err(err)
        }
      },
    }
  }

  #[inline]
  async fn can_send_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, Self::Error> {
    let result = self
      .get_collab_access_level(CollabUserId::UserId(uid), oid)
      .await;

    match result {
      Ok(level) => match level {
        AFAccessLevel::ReadOnly | AFAccessLevel::ReadAndComment => Ok(false),
        AFAccessLevel::ReadAndWrite | AFAccessLevel::FullAccess => Ok(true),
      },
      Err(err) => {
        // If the collab object with given oid is not found which means the collab object is created
        // by the user. So the user is allowed to send the message
        if err.is_record_not_found() {
          return Ok(true);
        }

        return Err(err);
      },
    }
  }

  #[inline]
  async fn can_receive_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, Self::Error> {
    Ok(
      self
        .get_collab_access_level(CollabUserId::UserId(uid), oid)
        .await
        .is_ok(),
    )
  }
}

#[derive(Clone)]
pub struct CollabHttpAccessControl<AC: CollabAccessControl>(pub Arc<AC>);

#[async_trait]
impl<AC> HttpAccessControlService for CollabHttpAccessControl<AC>
where
  AC: CollabAccessControl,
  AppError: From<<AC as CollabAccessControl>::Error>,
{
  fn resource(&self) -> AccessResource {
    AccessResource::Collab
  }

  async fn check_collab_permission(
    &self,
    oid: &str,
    user_uuid: &Uuid,
    method: Method,
    _path: Path<Url>,
  ) -> Result<(), AppError> {
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

#[derive(Clone)]
pub struct CollabStorageAccessControlImpl<CollabAC, WorkspaceAC> {
  pub(crate) collab_access_control: Arc<CollabAC>,
  pub(crate) workspace_access_control: Arc<WorkspaceAC>,
}

#[async_trait]
impl<CollabAC, WorkspaceAC> CollabStorageAccessControl
  for CollabStorageAccessControlImpl<CollabAC, WorkspaceAC>
where
  CollabAC: CollabAccessControl<Error = AppError>,
  WorkspaceAC: WorkspaceAccessControl,
{
  #[instrument(level = "trace", skip_all)]
  async fn get_collab_access_level(
    &self,
    uid: &i64,
    oid: &str,
  ) -> Result<AFAccessLevel, DatabaseError> {
    let level = self
      .collab_access_control
      .get_collab_access_level(uid.into(), oid)
      .await
      .map_err(|err| {
        to_database_error(
          err,
          format!(
            "failed to get the collab access level of user:{} for object:{}",
            uid, oid
          ),
        )
      })?;
    Ok(level)
  }

  async fn get_user_role(&self, uid: &i64, workspace_id: &str) -> Result<AFRole, DatabaseError> {
    self
      .workspace_access_control
      .get_role_from_uid(uid, &workspace_id.parse()?)
      .await
      .map_err(|err| {
        to_database_error(
          err,
          format!(
            "failed to get the role of the user:{} in workspace:{}",
            uid, workspace_id
          ),
        )
      })
  }
}

fn to_database_error(err: AppError, msg: String) -> DatabaseError {
  if err.is_record_not_found() {
    DatabaseError::RecordNotFound(msg)
  } else {
    DatabaseError::Internal(anyhow::Error::new(err))
  }
}
