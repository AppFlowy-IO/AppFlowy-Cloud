use anyhow::Error;
use async_trait::async_trait;

#[async_trait]
pub trait CollabPermission: Sync + Send + 'static {
  /// Return true if the user is allowed to send the message. This function will be called very
  /// frequently, so it should be very fast.
  async fn is_allowed_send_by_user(&self, uid: i64, oid: &str) -> Result<bool, Error>;

  /// Return true if the user is allowed to receive the message.This function will be called very
  /// frequently, so it should be very fast.
  async fn is_allowed_recv_by_user(&self, uid: i64, oid: &str) -> Result<bool, Error>;
}
