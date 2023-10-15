use anyhow::Error;
use async_trait::async_trait;

#[async_trait]
pub trait CollabPermission: Sync + Send + 'static {
  /// Return true if the user is allowed to send the message.
  async fn is_allowed_send_by_user(&self, uid: i64) -> Result<bool, Error>;

  /// Return true if the user is allowed to receive the message.
  async fn is_allowed_recv_by_user(&self, uid: i64) -> Result<bool, Error>;
}
