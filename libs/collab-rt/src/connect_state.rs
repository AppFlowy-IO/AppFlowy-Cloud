use crate::CollabClientStream;
use collab_rt_entity::message::{RealtimeMessage, SystemMessage};
use collab_rt_entity::user::{RealtimeUser, UserDevice};
use dashmap::DashMap;

use std::sync::Arc;
use tracing::{info, trace};

#[derive(Clone, Default)]
pub struct ConnectState {
  pub(crate) user_by_device: Arc<DashMap<UserDevice, RealtimeUser>>,
  /// Maintains a record of all client streams. A client stream associated with a user may be terminated for the following reasons:
  /// 1. User disconnection.
  /// 2. Server closes the connection due to a ping/pong timeout.
  pub(crate) client_stream_by_user: Arc<DashMap<RealtimeUser, CollabClientStream>>,
}

impl ConnectState {
  pub fn new() -> Self {
    Self::default()
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

  #[allow(dead_code)]
  fn num_connected_users(&self) -> usize {
    self.user_by_device.len()
  }

  #[allow(dead_code)]
  fn get_user_by_device(&self, user_device: &UserDevice) -> Option<RealtimeUser> {
    self.user_by_device.get(user_device).map(|v| v.clone())
  }
}

#[cfg(test)]
mod tests {
  use crate::connect_state::ConnectState;
  use crate::{CollabClientStream, RealtimeClientWebsocketSink};
  use collab_rt_entity::message::RealtimeMessage;
  use collab_rt_entity::user::{RealtimeUser, UserDevice};

  struct MockSink;

  impl RealtimeClientWebsocketSink for MockSink {
    fn do_send(&self, _message: RealtimeMessage) {}
  }

  fn mock_user(uid: i64, device_id: &str) -> RealtimeUser {
    RealtimeUser::new(
      uid,
      device_id.to_string(),
      uuid::Uuid::new_v4().to_string(),
      chrono::Utc::now().timestamp(),
    )
  }

  fn mock_stream() -> CollabClientStream {
    CollabClientStream::new(MockSink)
  }

  #[tokio::test]
  async fn same_user_different_device_connect_test() {
    let connect_state = ConnectState::new();
    let user_device_a = mock_user(1, "device_a");
    let user_device_b = mock_user(1, "device_b");
    connect_state.handle_user_connect(user_device_a, mock_stream());
    connect_state.handle_user_connect(user_device_b, mock_stream());

    assert_eq!(connect_state.num_connected_users(), 2);
  }

  #[tokio::test]
  async fn same_user_same_device_connect_test() {
    let connect_state = ConnectState::new();
    let user_device_a = mock_user(1, "device_a");
    let user_device_b = mock_user(1, "device_a");
    connect_state.handle_user_connect(user_device_a, mock_stream());
    connect_state.handle_user_connect(user_device_b.clone(), mock_stream());

    assert_eq!(connect_state.num_connected_users(), 1);
    let user = connect_state
      .get_user_by_device(&UserDevice::from(&user_device_b))
      .unwrap();
    assert_eq!(user, user_device_b);
  }

  #[tokio::test]
  async fn multiple_devices_connect_test() {
    let user_a = vec![
      mock_user(1, "device_a"),
      mock_user(1, "device_b"),
      mock_user(1, "device_c"),
      mock_user(1, "device_d"),
    ];

    let user_b = vec![
      mock_user(2, "device_a"),
      mock_user(2, "device_b"),
      mock_user(2, "device_b"),
      mock_user(2, "device_a"),
    ];

    let connect_state = ConnectState::new();

    let (tx, rx_1) = tokio::sync::oneshot::channel();
    let cloned_connect_state = connect_state.clone();
    tokio::spawn(async move {
      for user in user_a {
        cloned_connect_state.handle_user_connect(user, mock_stream());
      }
      tx.send(()).unwrap();
    });

    let (tx, rx_2) = tokio::sync::oneshot::channel();
    let cloned_connect_state = connect_state.clone();
    tokio::spawn(async move {
      for user in user_b {
        cloned_connect_state.handle_user_connect(user, mock_stream());
      }
      tx.send(()).unwrap();
    });

    let _ = futures::future::join(rx_1, rx_2).await;

    assert_eq!(connect_state.num_connected_users(), 6);
  }
}
