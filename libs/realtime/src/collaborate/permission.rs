use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::AFAccessLevel;
use reqwest::Method;

use std::sync::Arc;

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
pub trait CollabAccessControl: Sync + Send + 'static {
  /// Return the access level of the user in the collab
  async fn get_collab_access_level(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
  ) -> Result<AFAccessLevel, AppError>;

  /// Return true if the user from the HTTP request is allowed to access the collab object.
  /// This function will be called very frequently, so it should be very fast.
  ///  
  async fn can_access_http_method(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
    method: &Method,
  ) -> Result<bool, AppError>;

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
//
#[async_trait]
impl<T> CollabAccessControl for Arc<T>
where
  T: CollabAccessControl,
{
  async fn get_collab_access_level(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
  ) -> Result<AFAccessLevel, AppError> {
    self.as_ref().get_collab_access_level(user, oid).await
  }

  async fn can_access_http_method(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
    method: &Method,
  ) -> Result<bool, AppError> {
    self
      .as_ref()
      .can_access_http_method(user, oid, method)
      .await
  }

  async fn can_send_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    self.as_ref().can_send_collab_update(uid, oid).await
  }

  async fn can_receive_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    self.as_ref().can_receive_collab_update(uid, oid).await
  }
}
