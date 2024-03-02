use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::AFAccessLevel;
use reqwest::Method;

#[derive(Debug)]
pub enum CollabUserId<'a> {
  UserId(&'a i64),
  UserUuid(&'a uuid::Uuid),
}

impl<'a> From<&'a i64> for CollabUserId<'a> {
  fn from(uid: &'a i64) -> Self {
    CollabUserId::UserId(uid)
  }
}

impl<'a> From<&'a uuid::Uuid> for CollabUserId<'a> {
  fn from(uid: &'a uuid::Uuid) -> Self {
    CollabUserId::UserUuid(uid)
  }
}

#[async_trait]
pub trait RealtimeCollabAccessControl: Sync + Send + 'static {
  async fn update_access_level_policy(
    &self,
    uid: &i64,
    oid: &str,
    level: AFAccessLevel,
  ) -> Result<(), AppError>;

  /// Return true if the user is allowed to send the message.
  /// This function will be called very frequently, so it should be very fast.
  ///
  /// The user can send the message if:
  /// 1. user is the member of the collab object
  /// 2. the permission level of the user is `ReadAndWrite` or `FullAccess`
  /// 3. If the collab object is not found which means the collab object is created by the user.
  async fn can_send_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, AppError>;

  /// Return true if the user is allowed to send the message.
  /// This function will be called very frequently, so it should be very fast.
  ///
  /// The user can recv the message if the user is the member of the collab object
  async fn can_receive_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, AppError>;
}
