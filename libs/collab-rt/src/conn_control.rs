use crate::CollabClientStream;
use collab_rt_entity::message::{RealtimeMessage, SystemMessage};
use collab_rt_entity::user::{RealtimeUser, UserDevice};
use dashmap::DashMap;

use std::sync::Arc;
use tracing::{info, trace};

#[derive(Clone)]
pub struct ConnectControl {
  pub(crate) user_by_device: Arc<DashMap<UserDevice, RealtimeUser>>,
  /// Maintains a record of all client streams. A client stream associated with a user may be terminated for the following reasons:
  /// 1. User disconnection.
  /// 2. Server closes the connection due to a ping/pong timeout.
  pub(crate) client_stream_by_user: Arc<DashMap<RealtimeUser, CollabClientStream>>,
}

impl ConnectControl {
  pub fn new() -> Self {
    Self {
      user_by_device: Arc::new(DashMap::new()),
      client_stream_by_user: Arc::new(DashMap::new()),
    }
  }
  pub fn handle_user_connect(
    &self,
    new_user: RealtimeUser,
    client_stream: CollabClientStream,
  ) -> Option<RealtimeUser> {
    let old_user = self
      .user_by_device
      .insert(UserDevice::from(&new_user), new_user.clone());

    trace!(
      "[realtime]: new connection => {}, removing old: {:?}",
      new_user,
      old_user
    );

    if let Some(old_user) = &old_user {
      // Remove and retrieve the old client stream if it exists.
      if let Some((_, client_stream)) = self.client_stream_by_user.remove(old_user) {
        info!("Removing old stream for same user and device: {}", old_user);
        // Notify the old stream of the duplicate connection.
        client_stream
          .sink
          .do_send(RealtimeMessage::System(SystemMessage::DuplicateConnection));
      }
      // Remove the old user from all collaboration groups.
    }
    self.client_stream_by_user.insert(new_user, client_stream);

    old_user
  }

  pub fn handle_user_disconnect(
    &self,
    disconnect_user: &RealtimeUser,
  ) -> Option<(UserDevice, RealtimeUser)> {
    let user_device = UserDevice::from(disconnect_user);
    let was_removed = self
      .user_by_device
      .remove_if(&user_device, |_, existing_user| {
        existing_user.session_id == disconnect_user.session_id
      });

    if was_removed.is_some() && self.client_stream_by_user.remove(disconnect_user).is_some() {
      info!("remove client stream: {}", &disconnect_user);
    }

    was_removed
  }
}
