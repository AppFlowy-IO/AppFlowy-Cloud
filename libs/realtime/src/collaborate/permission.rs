use crate::entities::{ClientMessage, RealtimeUser};
use crate::error::RealtimeError;
use async_trait::async_trait;
use realtime_entity::collab_msg::CollabMessage;

#[async_trait]
pub trait CollabPermissionService {
  /// Return true if the user is allowed to send the message.
  async fn is_allowed_send_by_user(&self, uid: i64) -> Result<(), RealtimeError> {
    todo!()
  }

  /// Return true if the user is allowed to receive the message.
  async fn is_allowed_recv_by_user(&self, uid: i64) -> Result<(), RealtimeError> {
    Ok(())
  }
}
