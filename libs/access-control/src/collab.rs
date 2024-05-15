use crate::act::Action;
use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::AFAccessLevel;

#[async_trait]
pub trait CollabAccessControl: Sync + Send + 'static {
  async fn enforce_action(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
    action: Action,
  ) -> Result<bool, AppError>;

  async fn enforce_access_level(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
    access_level: AFAccessLevel,
  ) -> Result<bool, AppError>;

  /// Return the access level of the user in the collab
  async fn update_access_level_policy(
    &self,
    uid: &i64,
    oid: &str,
    level: AFAccessLevel,
  ) -> Result<(), AppError>;

  async fn remove_access_level(&self, uid: &i64, oid: &str) -> Result<(), AppError>;
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
    workspace_id: &str,
    uid: &i64,
    oid: &str,
  ) -> Result<bool, AppError>;

  /// Return true if the user is allowed to observe the changes of given collab.
  /// This function will be called very frequently, so it should be very fast.
  ///
  /// The user can recv the message if the user is the member of the collab object
  async fn can_read_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
  ) -> Result<bool, AppError>;
}
