use crate::act::Action;
use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::AFAccessLevel;
use uuid::Uuid;

#[async_trait]
pub trait CollabAccessControl: Sync + Send + 'static {
  /// Check if the user can perform the action on the collab.
  /// Returns AppError::NotEnoughPermission if the user does not have the permission.
  async fn enforce_action(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    oid: &Uuid,
    action: Action,
  ) -> Result<(), AppError>;

  /// Check if the user has the access level in the collab.
  /// Returns AppError::NotEnoughPermission if the user does not have the access level.
  async fn enforce_access_level(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    oid: &Uuid,
    access_level: AFAccessLevel,
  ) -> Result<(), AppError>;

  /// Return the access level of the user in the collab
  async fn update_access_level_policy(
    &self,
    uid: &i64,
    oid: &Uuid,
    level: AFAccessLevel,
  ) -> Result<(), AppError>;

  async fn remove_access_level(&self, uid: &i64, oid: &Uuid) -> Result<(), AppError>;
}

#[async_trait]
pub trait RealtimeAccessControl: Sync + Send + 'static {
  /// Return true if the user is allowed to edit collab.
  /// This function will be called very frequently, so it should be very fast.
  ///
  /// The user can send the message if:
  /// 1. user is the member of the collab object
  /// 2. the permission level of the user is `ReadAndWrite` or `FullAccess`
  /// 3. If the collab object is not found which means the collab object is created by the user.
  async fn can_write_collab(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    oid: &Uuid,
  ) -> Result<bool, AppError>;

  /// Return true if the user is allowed to observe the changes of given collab.
  /// This function will be called very frequently, so it should be very fast.
  ///
  /// The user can recv the message if the user is the member of the collab object
  async fn can_read_collab(
    &self,
    workspace_id: &Uuid,
    uid: &i64,
    oid: &Uuid,
  ) -> Result<bool, AppError>;
}
