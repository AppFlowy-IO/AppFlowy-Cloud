use assert_json_diff::assert_json_eq;
use client_api::collab_sync::{SinkConfig, SyncObject, SyncPlugin};
use client_api::ws::{BusinessID, ConnectState, WSClient, WSClientConfig};
use collab::core::collab::MutexCollab;
use collab::core::collab_state::SyncState;
use collab::core::origin::{CollabClient, CollabOrigin};
use collab::preclude::Collab;
use collab_define::CollabType;
use serde_json::Value;
use sqlx::types::Uuid;
use std::collections::HashMap;
use std::sync::{Arc, Once};
use storage_entity::QueryCollabParams;
use tokio::time::{timeout, Duration};
use tokio_stream::StreamExt;
use tracing_subscriber::fmt::Subscriber;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::client::utils::{generate_unique_registered_user, User};
use crate::client_api_client;

pub(crate) struct TestClient {
  pub ws_client: WSClient,
  pub api_client: client_api::Client,
  pub collab_by_object_id: HashMap<String, TestCollab>,
  device_id: String,
}

pub(crate) struct TestCollab {
  #[allow(dead_code)]
  pub origin: CollabOrigin,
  pub collab: Arc<MutexCollab>,
}

impl TestClient {
  pub(crate) async fn new_user() -> Self {
    let registered_user = generate_unique_registered_user().await;
    let device_id = Uuid::new_v4().to_string();
    Self::new(device_id, registered_user).await
  }

  pub(crate) async fn user_with_new_device(registered_user: User) -> Self {
    let device_id = Uuid::new_v4().to_string();
    Self::new(device_id, registered_user).await
  }

  pub(crate) async fn new(device_id: String, registered_user: User) -> Self {
    setup_log();
    let api_client = client_api_client();
    api_client
      .sign_in_password(&registered_user.email, &registered_user.password)
      .await
      .unwrap();

    // Connect to server via websocket
    let ws_client = WSClient::new(WSClientConfig {
      buffer_capacity: 100,
      ping_per_secs: 6,
      retry_connect_per_pings: 5,
    });
    ws_client
      .connect(api_client.ws_url(&device_id).unwrap())
      .await
      .unwrap();
    Self {
      ws_client,
      api_client,
      collab_by_object_id: Default::default(),
      device_id,
    }
  }

  pub(crate) async fn wait_object_sync_complete(&self, object_id: &str) {
    let mut sync_state = self
      .collab_by_object_id
      .get(object_id)
      .unwrap()
      .collab
      .lock()
      .subscribe_sync_state();

    const TIMEOUT_DURATION: Duration = Duration::from_secs(10);
    while let Ok(Some(state)) = timeout(TIMEOUT_DURATION, sync_state.next()).await {
      if state == SyncState::SyncFinished {
        break;
      }
    }
  }

  #[allow(dead_code)]
  pub(crate) async fn wait_ws_connected(&self) {
    let mut connect_state = self.ws_client.subscribe_connect_state().await;

    const TIMEOUT_DURATION: Duration = Duration::from_secs(10);
    while let Ok(Ok(state)) = timeout(TIMEOUT_DURATION, connect_state.recv()).await {
      if state == ConnectState::Connected {
        break;
      }
    }
  }

  pub(crate) async fn current_workspace_id(&self) -> String {
    self
      .api_client
      .workspaces()
      .await
      .unwrap()
      .first()
      .unwrap()
      .workspace_id
      .to_string()
  }

  pub(crate) async fn create_collab(
    &mut self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) {
    let uid = self.api_client.profile().await.unwrap().uid.unwrap();

    // Subscribe to object
    let handler = self
      .ws_client
      .subscribe(BusinessID::CollabId, object_id.to_string())
      .await
      .unwrap();
    let (sink, stream) = (handler.sink(), handler.stream());
    let origin = CollabOrigin::Client(CollabClient::new(uid, self.device_id.clone()));
    let collab = Arc::new(MutexCollab::new(origin.clone(), object_id, vec![]));

    let ws_connect_state = self.ws_client.subscribe_connect_state().await;
    let object = SyncObject::new(object_id, workspace_id, collab_type, &self.device_id);
    let sync_plugin = SyncPlugin::new(
      origin.clone(),
      object,
      Arc::downgrade(&collab),
      sink,
      SinkConfig::default(),
      stream,
      Some(handler),
      ws_connect_state,
    );

    collab.lock().add_plugin(Arc::new(sync_plugin));
    collab.async_initialize().await;
    let test_collab = TestCollab { origin, collab };
    self
      .collab_by_object_id
      .insert(object_id.to_string(), test_collab);
  }

  pub(crate) async fn disconnect(&self) {
    self.ws_client.disconnect().await;
  }

  pub(crate) async fn reconnect(&self) {
    self
      .ws_client
      .connect(self.api_client.ws_url(&self.device_id).unwrap())
      .await
      .unwrap();
  }
}

pub async fn assert_remote_collab(
  client: &mut client_api::Client,
  object_id: &str,
  collab_type: &CollabType,
  secs: u64,
  expected: Value,
) {
  let collab_type = collab_type.clone();
  let object_id = object_id.to_string();
  let mut retry_count = 0;

  loop {
    tokio::select! {
       _ = tokio::time::sleep(Duration::from_secs(secs)) => {
         panic!("Query collab timeout");
       },
       result = client.get_collab(QueryCollabParams {
         object_id: object_id.clone(),
         collab_type: collab_type.clone(),
       }) => {
        retry_count += 1;
        match result {
          Ok(data) => {
            let json = Collab::new_with_raw_data(CollabOrigin::Empty, &object_id, vec![data.to_vec()], vec![]).unwrap().to_json_value();
            if retry_count > 10 {
              assert_json_eq!(json, expected);
               break;
            }

            if json == expected {
              break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
          },
          Err(e) => {
            if retry_count > 5 {
              panic!("Query collab failed: {}", e);
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
          }
        }
       },
    }
  }
}

pub(crate) async fn assert_client_collab(
  client: &mut TestClient,
  object_id: &str,
  secs: u64,
  expected: Value,
) {
  let object_id = object_id.to_string();
  let mut retry_count = 0;
  loop {
    tokio::select! {
       _ = tokio::time::sleep(Duration::from_secs(secs)) => {
         panic!("timeout");
       },
       json = async {
        client
          .collab_by_object_id
          .get_mut(&object_id)
          .unwrap()
          .collab
          .lock()
          .to_json_value()
      } => {
        retry_count += 1;
        if retry_count > 20 {
          assert_json_eq!(json, expected);
          break;
        }

        if json == expected {
          break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
      }
    }
  }
}

#[allow(dead_code)]
pub async fn get_collab_json_from_server(
  client: &mut client_api::Client,
  object_id: &str,
  collab_type: CollabType,
) -> serde_json::Value {
  let bytes = client
    .get_collab(QueryCollabParams {
      object_id: object_id.to_string(),
      collab_type,
    })
    .await
    .unwrap();

  Collab::new_with_raw_data(CollabOrigin::Empty, object_id, vec![bytes.to_vec()], vec![])
    .unwrap()
    .to_json_value()
}

pub fn setup_log() {
  static START: Once = Once::new();
  START.call_once(|| {
    let level = std::env::var("RUST_LOG").unwrap_or("trace".to_string());
    let mut filters = vec![];
    filters.push(format!("client_api={}", level));
    std::env::set_var("RUST_LOG", filters.join(","));

    let subscriber = Subscriber::builder()
      .with_env_filter(EnvFilter::from_default_env())
      .with_ansi(true)
      .finish();
    subscriber.try_init().unwrap();
  });
}
