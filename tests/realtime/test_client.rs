use crate::client::utils::REGISTERED_USERS_MUTEX;

use collab::core::collab::MutexCollab;
use collab::core::origin::{CollabClient, CollabOrigin};

use collab_plugins::sync_plugin::{SyncObject, SyncPlugin};

use collab::preclude::Collab;
use collab_define::CollabType;
use sqlx::types::Uuid;
use std::sync::Arc;

use crate::user_1_signed_in;
use assert_json_diff::assert_json_eq;
use client_api::ws::{BusinessID, WSClient, WSClientConfig, WebSocketChannel};
use serde_json::Value;
use std::time::Duration;
use storage_entity::QueryCollabParams;

pub(crate) struct TestClient {
  pub ws_client: WSClient,
  #[allow(dead_code)]
  pub origin: CollabOrigin,
  pub collab: Arc<MutexCollab>,
  #[allow(dead_code)]
  pub handler: Arc<WebSocketChannel>,
  pub api_client: client_api::Client,
  device_id: String,
}

impl TestClient {
  pub(crate) async fn new_with_device_id(
    object_id: &str,
    device_id: &str,
    collab_type: CollabType,
  ) -> Self {
    let device_id = device_id.to_string();
    let _guard = REGISTERED_USERS_MUTEX.lock().await;

    // Sign in
    let mut api_client = user_1_signed_in().await;

    // Connect to server via websocket
    let ws_client = WSClient::new(WSClientConfig {
      buffer_capacity: 100,
      ping_per_secs: 2,
      retry_connect_per_pings: 5,
    });
    ws_client
      .connect(api_client.ws_url(&device_id).unwrap())
      .await
      .unwrap();

    // Get workspace id and uid
    let workspace_id = api_client
      .workspaces()
      .await
      .unwrap()
      .first()
      .unwrap()
      .workspace_id
      .to_string();
    let uid = api_client.profile().await.unwrap().uid.unwrap();

    // Subscribe to object
    let handler = ws_client
      .subscribe(BusinessID::CollabId, object_id.to_string())
      .await
      .unwrap();
    let (sink, stream) = (handler.sink(), handler.stream());
    let origin = CollabOrigin::Client(CollabClient::new(uid, device_id.clone()));
    let collab = Arc::new(MutexCollab::new(origin.clone(), object_id, vec![]));

    let object = SyncObject::new(object_id, &workspace_id, collab_type);
    let sync_plugin = SyncPlugin::new(
      origin.clone(),
      object,
      Arc::downgrade(&collab),
      sink,
      stream,
    );
    collab.lock().add_plugin(Arc::new(sync_plugin));
    collab.async_initialize().await;

    Self {
      ws_client,
      api_client,
      origin,
      collab,
      handler,
      device_id,
    }
  }

  pub(crate) async fn new(object_id: &str, collab_type: CollabType) -> Self {
    let device_id = Uuid::new_v4().to_string();
    Self::new_with_device_id(object_id, &device_id, collab_type).await
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

#[allow(dead_code)]
pub async fn assert_collab_json(
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
            if retry_count > 5 {
              assert_json_eq!(json, expected);
               break;
            }

            if json == expected {
              break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
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
