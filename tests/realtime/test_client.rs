use crate::client::utils::{REGISTERED_EMAIL, REGISTERED_PASSWORD, REGISTERED_USER_MUTEX};

use client_api::Client;
use collab::core::collab::MutexCollab;
use collab::core::origin::{CollabClient, CollabOrigin};

use collab_plugins::sync_plugin::{SyncObject, SyncPlugin};
use collab_ws::{WSClient, WSClientConfig, WSObjectHandler};

use collab::preclude::Collab;
use collab_define::CollabType;
use std::sync::Arc;

use serde_json::Value;
use std::time::Duration;
use storage_entity::QueryCollabParams;

pub(crate) struct TestClient {
  pub ws_client: WSClient,
  #[allow(dead_code)]
  pub origin: CollabOrigin,
  pub collab: Arc<MutexCollab>,
  #[allow(dead_code)]
  pub handler: Arc<WSObjectHandler>,
}

impl TestClient {
  pub(crate) async fn new(client: &mut Client, object_id: &str, collab_type: CollabType) -> Self {
    let _guard = REGISTERED_USER_MUTEX.lock().await;

    // Sign in
    client
      .sign_in_password(&REGISTERED_EMAIL, &REGISTERED_PASSWORD)
      .await
      .unwrap();

    // Connect to server via websocket
    let ws_client = WSClient::new(
      client.ws_url().unwrap(),
      WSClientConfig {
        buffer_capacity: 100,
        ping_per_secs: 2,
        retry_connect_per_pings: 5,
      },
    );
    ws_client.connect().await.unwrap();

    // Get workspace id and uid
    let workspace_id = client
      .workspaces()
      .await
      .unwrap()
      .first()
      .unwrap()
      .workspace_id
      .to_string();
    let uid = client.profile().await.unwrap().uid.unwrap();

    // Subscribe to object
    let handler = ws_client.subscribe(1, object_id.to_string()).await.unwrap();
    let (sink, stream) = (handler.sink(), handler.stream());
    let origin = CollabOrigin::Client(CollabClient::new(uid, "1"));
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
      origin,
      collab,
      handler,
    }
  }

  pub(crate) async fn disconnect(&self) {
    self.ws_client.disconnect().await;
  }
}

#[allow(dead_code)]
pub async fn assert_collab_json(
  client: &client_api::Client,
  object_id: &str,
  collab_type: &CollabType,
  timeout: u64,
  expected: Value,
) {
  let collab_type = collab_type.clone();
  let object_id = object_id.to_string();

  loop {
    tokio::select! {
       _ = tokio::time::sleep(Duration::from_secs(timeout)) => {
         panic!("Query collab timeout");
       },
       result = client.get_collab(QueryCollabParams {
         object_id: object_id.clone(),
         collab_type: collab_type.clone(),
       }) => {
        match result {
          Ok(data) => {
            let json = Collab::new_with_raw_data(CollabOrigin::Empty, &object_id, vec![data.to_vec()], vec![]).unwrap().to_json_value();
            if json == expected {
              break;
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
          },
          Err(_) => {
            tokio::time::sleep(Duration::from_millis(200)).await;
          }
        }
       },
    }
  }
}

#[allow(dead_code)]
pub async fn get_collab_json_from_server(
  client: &client_api::Client,
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
