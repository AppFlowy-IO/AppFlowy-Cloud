use app_error::AppError;
use async_trait::async_trait;

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
pub trait RealtimeAccessControl: Sync + Send + 'static {
  /// Return true if the user is allowed to edit collab.
  /// This function will be called very frequently, so it should be very fast.
  ///
  /// The user can send the message if:
  /// 1. user is the member of the collab object
  /// 2. the permission level of the user is `ReadAndWrite` or `FullAccess`
  /// 3. If the collab object is not found which means the collab object is created by the user.
  async fn can_write_collab(&self, uid: &i64, oid: &str) -> Result<bool, AppError>;

  /// Return true if the user is allowed to observe the changes of given collab.
  /// This function will be called very frequently, so it should be very fast.
  ///
  /// The user can recv the message if the user is the member of the collab object
  async fn can_read_collab(&self, uid: &i64, oid: &str) -> Result<bool, AppError>;
}
