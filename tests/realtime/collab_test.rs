use crate::client::utils::{REGISTERED_EMAIL, REGISTERED_PASSWORD};
use crate::client_api_client;
use serde_json::json;

use client_api::Client;
use collab::core::collab::MutexCollab;
use collab::core::origin::{CollabClient, CollabOrigin};
use collab_define::CollabType;
use collab_plugins::sync_plugin::{SyncObject, SyncPlugin};
use collab_ws::{WSClient, WSClientConfig, WSObjectHandler};

use assert_json_diff::assert_json_eq;
use collab::preclude::Collab;
use std::sync::Arc;
use std::time::Duration;
use storage_entity::QueryCollabParams;
#[tokio::test]
async fn realtime_write_test() {
  let mut c = client_api_client();
  let object_id = uuid::Uuid::new_v4().to_string();
  let test_client = TestClient::new(&mut c, &object_id).await;
  for i in 0..=10 {
    test_client
      .collab
      .lock()
      .insert(&i.to_string(), i.to_string());
  }
  tokio::time::sleep(Duration::from_secs(1)).await;
  // when disconnect, the collab data will be persisted
  test_client.disconnect().await;
  tokio::time::sleep(Duration::from_secs(3)).await;

  // check if the data is persisted
  let bytes = c
    .get_collab(QueryCollabParams {
      object_id: object_id.clone(),
      collab_type: CollabType::Document,
    })
    .await
    .unwrap();

  // check if the data is correct
  let json = Collab::new_with_raw_data(
    CollabOrigin::Empty,
    &object_id,
    vec![bytes.to_vec()],
    vec![],
  )
  .unwrap()
  .to_json_value();
  assert_json_eq!(
    json,
    json!( {
      "0": "0",
      "1": "1",
      "10": "10",
      "2": "2",
      "3": "3",
      "4": "4",
      "5": "5",
      "6": "6",
      "7": "7",
      "8": "8",
      "9": "9"
    })
  );
}

pub(crate) struct TestClient {
  pub ws_client: WSClient,
  #[allow(dead_code)]
  pub origin: CollabOrigin,
  pub collab: Arc<MutexCollab>,
  #[allow(dead_code)]
  pub handler: Arc<WSObjectHandler>,
}

impl TestClient {
  async fn new(client: &mut Client, object_id: &str) -> Self {
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

    let object = SyncObject::new(object_id, &workspace_id);
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

  async fn disconnect(&self) {
    self.ws_client.disconnect().await;
  }
}
