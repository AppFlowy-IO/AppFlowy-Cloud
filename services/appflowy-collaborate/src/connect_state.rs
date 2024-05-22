use collab_rt_entity::user::{RealtimeUser, UserDevice};
use collab_rt_entity::{RealtimeMessage, SystemMessage};
use dashmap::DashMap;

use crate::client::client_msg_router::ClientMessageRouter;
use dashmap::mapref::entry::Entry;
use std::sync::Arc;
use tracing::{info, trace};

#[derive(Clone, Default)]
pub struct ConnectState {
  pub(crate) user_by_device: Arc<DashMap<UserDevice, RealtimeUser>>,
  /// Maintains a record of all client streams. A client stream associated with a user may be terminated for the following reasons:
  /// 1. User disconnection.
  /// 2. Server closes the connection due to a ping/pong timeout.
  pub(crate) client_message_routers: Arc<DashMap<RealtimeUser, ClientMessageRouter>>,
}

impl ConnectState {
  pub fn new() -> Self {
    Self::default()
  }

  /// Handles a new user connection, updating the connection state accordingly.
  ///
  /// This function checks if there is already a connection from the same user device. If an existing
  /// connection is found and the new connection is more recent (`connect_at` is greater), the old connection
  /// is replaced with the new one. This process involves:
  ///
  /// - Removing the old user's client stream, if present, and sending a `DuplicateConnection` system message.
  /// - Inserting the new user connection into the `user_by_device` and `client_stream_by_user` maps.
  ///
  pub fn handle_user_connect(
    &self,
    new_user: RealtimeUser,
    client_message_router: ClientMessageRouter,
  ) -> Option<RealtimeUser> {
    let user_device = UserDevice::from(&new_user);
    let entry = self.user_by_device.entry(user_device);
    match entry {
      Entry::Occupied(mut e) => {
        if e.get().connect_at <= new_user.connect_at {
          let old_user = e.insert(new_user.clone());
          trace!("[realtime]: new connection replaces old => {}", new_user);
          if let Some((_, old_stream)) = self.client_message_routers.remove(&old_user) {
            info!(
              "Removing old stream for same user and device: {}",
              old_user.uid
            );
            old_stream
              .sink
              .do_send(RealtimeMessage::System(SystemMessage::DuplicateConnection));
          }
          self
            .client_message_routers
            .insert(new_user, client_message_router);
          Some(old_user)
        } else {
          None
        }
      },
      Entry::Vacant(e) => {
        trace!("[realtime]: new connection => {}", new_user);
        e.insert(new_user.clone());
        self
          .client_message_routers
          .insert(new_user, client_message_router);
        None
      },
    }
  }

  /// Handles the disconnection of a user from the system.
  ///
  /// remove a user based on their device and session ID. If the session ID of the disconnecting user matches
  /// the session ID stored in the system for that device, the user is removed. Additionally, it also
  /// attempts to remove the associated client stream for the disconnecting user.
  ///
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

    if was_removed.is_some()
      && self
        .client_message_routers
        .remove(disconnect_user)
        .is_some()
    {
      info!("remove client stream: {}", &disconnect_user);
    }

    was_removed
  }

  pub fn number_of_connected_users(&self) -> usize {
    self.user_by_device.len()
  }

  #[allow(dead_code)]
  fn get_user_by_device(&self, user_device: &UserDevice) -> Option<RealtimeUser> {
    self.user_by_device.get(user_device).map(|v| v.clone())
  }
}

#[cfg(test)]
mod tests {
  use crate::client::client_msg_router::ClientMessageRouter;
  use crate::connect_state::ConnectState;
  use crate::RealtimeClientWebsocketSink;
  use collab_rt_entity::user::{RealtimeUser, UserDevice};
  use collab_rt_entity::RealtimeMessage;
  use std::time::Duration;
  use tokio::time::sleep;

  struct MockSink;

  impl RealtimeClientWebsocketSink for MockSink {
    fn do_send(&self, _message: RealtimeMessage) {}
  }

  fn mock_user(uid: i64, device_id: &str, connect_at: i64) -> RealtimeUser {
    RealtimeUser::new(
      uid,
      device_id.to_string(),
      uuid::Uuid::new_v4().to_string(),
      connect_at,
      "0.5.8".to_string(),
    )
  }

  fn mock_stream() -> ClientMessageRouter {
    ClientMessageRouter::new(MockSink)
  }

  #[tokio::test]
  async fn same_user_different_device_connect_test() {
    let connect_state = ConnectState::new();
    let user_device_a = mock_user(1, "device_a", 1);
    let user_device_b = mock_user(1, "device_b", 1);
    connect_state.handle_user_connect(user_device_a, mock_stream());
    connect_state.handle_user_connect(user_device_b, mock_stream());

    assert_eq!(connect_state.user_by_device.len(), 2);
  }

  #[tokio::test]
  async fn same_user_same_device_connect_test() {
    let connect_state = ConnectState::new();
    let user_device_a = mock_user(1, "device_a", 1);
    let user_device_b = mock_user(1, "device_a", 1);
    connect_state.handle_user_connect(user_device_a, mock_stream());
    connect_state.handle_user_connect(user_device_b.clone(), mock_stream());

    assert_eq!(connect_state.user_by_device.len(), 1);
    let user = connect_state
      .get_user_by_device(&UserDevice::from(&user_device_b))
      .unwrap();
    assert_eq!(user, user_device_b);
  }

  #[tokio::test]
  async fn multiple_same_devices_connect_test() {
    let connect_state = ConnectState::new();
    let mut handles = vec![];
    for i in 0..1000 {
      let cloned_connect_state = connect_state.clone();
      let handle = tokio::spawn(async move {
        let random_seconds = rand::random::<u64>() % 500;
        sleep(Duration::from_millis(random_seconds)).await;
        let user = mock_user(1, "device_a", i);
        cloned_connect_state.handle_user_connect(user, mock_stream());
      });
      handles.push(handle);
    }

    let _ = futures::future::join_all(handles).await;
    let user = connect_state
      .get_user_by_device(&UserDevice::new("device_a", 1))
      .unwrap();
    assert_eq!(connect_state.user_by_device.len(), 1);
    assert_eq!(connect_state.client_message_routers.len(), 1);
    assert_eq!(user.connect_at, 999);
  }

  #[tokio::test]
  async fn multiple_same_devices_connect_disconnect_test() {
    let connect_state = ConnectState::new();
    let mut handles = vec![];
    for i in 0..2000 {
      let should_disconnect = i % 2 == 0;
      let cloned_connect_state = connect_state.clone();
      let handle = tokio::spawn(async move {
        let random_seconds = rand::random::<u64>() % 500;
        sleep(Duration::from_millis(random_seconds)).await;
        let user = mock_user(1, "device_a", i);

        if should_disconnect {
          cloned_connect_state.handle_user_disconnect(&user);
        } else {
          cloned_connect_state.handle_user_connect(user, mock_stream());
        }
      });
      handles.push(handle);
    }

    let _ = futures::future::join_all(handles).await;
    let user = connect_state
      .get_user_by_device(&UserDevice::new("device_a", 1))
      .unwrap();

    assert_eq!(connect_state.user_by_device.len(), 1);
    assert_eq!(connect_state.client_message_routers.len(), 1);
    assert_eq!(user.connect_at, 1999);
  }

  #[tokio::test]
  async fn multiple_devices_connect_test() {
    let user_a = vec![
      mock_user(1, "device_a", 1),
      mock_user(1, "device_b", 2),
      mock_user(1, "device_c", 1),
      mock_user(1, "device_d", 1),
    ];

    let user_b = vec![
      mock_user(2, "device_a", 1),
      // device_b starts two connections, last connection should be kept.
      mock_user(2, "device_b", 1),
      mock_user(2, "device_b", 2),
      mock_user(2, "device_a", 1),
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
    let clone_user_b = user_b.clone();
    tokio::spawn(async move {
      for user in clone_user_b {
        cloned_connect_state.handle_user_connect(user, mock_stream());
      }
      tx.send(()).unwrap();
    });

    let _ = futures::future::join(rx_1, rx_2).await;
    assert_eq!(connect_state.user_by_device.len(), 6);

    // device_b with connect_at 2 should be kept.
    let user = connect_state
      .get_user_by_device(&UserDevice::from(&user_b[2]))
      .unwrap();
    assert_eq!(user.connect_at, 2);
  }
}
