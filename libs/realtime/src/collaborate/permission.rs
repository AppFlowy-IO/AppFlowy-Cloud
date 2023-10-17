use async_trait::async_trait;
use database_entity::AFAccessLevel;
use reqwest::Method;
use std::fmt::Display;

#[derive(Debug)]
pub enum CollabUserId<'a> {
  UserId(&'a i64),
  UserUuid(&'a uuid::Uuid),
}

#[async_trait]
pub trait CollabPermission: Sync + Send + 'static {
  type Error: Display;

  /// Return the access level of the user in the collab
  /// If the collab object is not found, return None. Otherwise, return the access level of the user
  async fn get_access_level(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
  ) -> Result<Option<AFAccessLevel>, Self::Error>;

  /// Return true if the user from the HTTP request is allowed to access the collab object.
  /// This function will be called very frequently, so it should be very fast.
  ///  
  async fn can_access_http_method(
    &self,
    user: CollabUserId<'_>,
    oid: &str,
    method: &Method,
  ) -> Result<bool, Self::Error>;

  /// Return true if the user is allowed to send the message.
  /// This function will be called very frequently, so it should be very fast.
  ///
  /// The user can send the message if:
  /// 1. user is the member of the collab object
  /// 2. the permission level of the user is `ReadAndWrite` or `FullAccess`
  async fn can_send_message(&self, uid: &i64, oid: &str) -> Result<bool, Self::Error>;

  /// Return true if the user is allowed to send the message.
  /// This function will be called very frequently, so it should be very fast.
  ///
  /// The user can recv the message if the user is the member of the collab object
  async fn can_receive_message(&self, uid: &i64, oid: &str) -> Result<bool, Self::Error>;
}
