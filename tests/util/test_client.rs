use assert_json_diff::assert_json_eq;
use client_api::collab_sync::{SinkConfig, SyncObject, SyncPlugin};
use client_api::ws::{BusinessID, ConnectState, WSClient, WSClientConfig};
use collab::core::collab::MutexCollab;
use collab::core::collab_state::SyncState;
use collab::core::origin::{CollabClient, CollabOrigin};
use collab::preclude::Collab;
use collab_entity::CollabType;
use database_entity::{
  AFAccessLevel, AFRole, InsertCollabMemberParams, QueryCollabParams, UpdateCollabMemberParams,
};
use serde_json::Value;
use shared_entity::dto::workspace_dto::CreateWorkspaceMember;
use sqlx::types::Uuid;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::time::{timeout, Duration};
use tokio_stream::StreamExt;

use crate::localhost_client;
use crate::user::utils::{generate_unique_registered_user, User};
use crate::util::setup_log;

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

  pub(crate) async fn add_client_as_workspace_member(
    &self,
    workspace_id: &str,
    other_client: &TestClient,
    role: AFRole,
  ) {
    let profile = other_client.api_client.get_profile().await.unwrap();
    let email = profile.email.unwrap();
    self
      .api_client
      .add_workspace_members(workspace_id, vec![CreateWorkspaceMember { email, role }])
      .await
      .unwrap();
  }

  pub(crate) async fn add_client_as_collab_member(
    &self,
    workspace_id: &str,
    object_id: &str,
    other_client: &TestClient,
    access_level: AFAccessLevel,
  ) {
    let uid = other_client
      .api_client
      .get_profile()
      .await
      .unwrap()
      .uid
      .unwrap();
    self
      .api_client
      .add_collab_member(InsertCollabMemberParams {
        uid,
        workspace_id: workspace_id.to_string(),
        object_id: object_id.to_string(),
        access_level,
      })
      .await
      .unwrap();
  }

  pub(crate) async fn update_collab_member_access_level(
    &self,
    workspace_id: &str,
    object_id: &str,
    other_client: &TestClient,
    access_level: AFAccessLevel,
  ) {
    let uid = other_client
      .api_client
      .get_profile()
      .await
      .unwrap()
      .uid
      .unwrap();
    self
      .api_client
      .update_collab_member(UpdateCollabMemberParams {
        uid,
        workspace_id: workspace_id.to_string(),
        object_id: object_id.to_string(),
        access_level,
      })
      .await
      .unwrap();
  }

  pub(crate) async fn user_with_new_device(registered_user: User) -> Self {
    let device_id = Uuid::new_v4().to_string();
    Self::new(device_id, registered_user).await
  }

  pub(crate) async fn new(device_id: String, registered_user: User) -> Self {
    setup_log();
    let api_client = localhost_client();
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
    self
      .wait_object_sync_complete_with_secs(object_id, 20)
      .await;
  }

  pub(crate) async fn wait_object_sync_complete_with_secs(&self, object_id: &str, secs: u64) {
    let mut sync_state = self
      .collab_by_object_id
      .get(object_id)
      .unwrap()
      .collab
      .lock()
      .subscribe_sync_state();

    let duration = Duration::from_secs(secs);
    while let Ok(Some(state)) = timeout(duration, sync_state.next()).await {
      if state == SyncState::SyncFinished {
        break;
      }
    }
  }

  #[allow(dead_code)]
  pub(crate) async fn wait_ws_connected(&self) {
    let mut connect_state = self.ws_client.subscribe_connect_state();

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
      .get_workspaces()
      .await
      .unwrap()
      .first()
      .unwrap()
      .workspace_id
      .to_string()
  }
  #[allow(clippy::await_holding_lock)]
  pub(crate) async fn create_collab(
    &mut self,
    workspace_id: &str,
    collab_type: CollabType,
  ) -> String {
    let object_id = Uuid::new_v4().to_string();
    let uid = self.api_client.get_profile().await.unwrap().uid.unwrap();

    // Subscribe to object
    let handler = self
      .ws_client
      .subscribe(BusinessID::CollabId, object_id.clone())
      .unwrap();
    let (sink, stream) = (handler.sink(), handler.stream());
    let origin = CollabOrigin::Client(CollabClient::new(uid, self.device_id.clone()));
    let collab = Arc::new(MutexCollab::new(origin.clone(), &object_id, vec![]));

    let ws_connect_state = self.ws_client.subscribe_connect_state();
    let object = SyncObject::new(&object_id, workspace_id, collab_type, &self.device_id);
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
    collab.lock().initialize().await;
    let test_collab = TestCollab { origin, collab };
    self
      .collab_by_object_id
      .insert(object_id.to_string(), test_collab);

    self.wait_object_sync_complete(&object_id).await;
    object_id
  }

  pub(crate) async fn open_workspace(&mut self, workspace_id: &str) {
    self
      .open_collab(workspace_id, workspace_id, CollabType::Folder)
      .await;
  }

  #[allow(clippy::await_holding_lock)]
  pub(crate) async fn open_collab(
    &mut self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) {
    let uid = self.api_client.get_profile().await.unwrap().uid.unwrap();

    // Subscribe to object
    let handler = self
      .ws_client
      .subscribe(BusinessID::CollabId, object_id.to_string())
      .unwrap();
    let (sink, stream) = (handler.sink(), handler.stream());
    let origin = CollabOrigin::Client(CollabClient::new(uid, self.device_id.clone()));
    let collab = Arc::new(MutexCollab::new(origin.clone(), object_id, vec![]));

    let ws_connect_state = self.ws_client.subscribe_connect_state();
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
    collab.lock().initialize().await;
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

pub async fn assert_server_collab(
  workspace_id: &str,
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
        workspace_id: workspace_id.to_string(),
         collab_type: collab_type.clone(),
       }) => {
        retry_count += 1;
        match &result {
          Ok(data) => {
            let json = Collab::new_with_raw_data(CollabOrigin::Empty, &object_id, vec![data.to_vec()], vec![]).unwrap().to_json_value();
            if retry_count > 10 {
              dbg!(workspace_id, object_id);
              assert_json_eq!(json, expected);
              break;
            }

            if json == expected {
              dbg!(workspace_id, object_id);
              break;
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
          },
          Err(e) => {
            if retry_count > 10 {
              panic!("Query collab failed: {}", e);
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
          }
        }
       },
    }
  }
}

pub(crate) async fn assert_client_collab(
  client: &mut TestClient,
  object_id: &str,
  expected: Value,
  _retry_duration: u64,
) {
  let secs = 30;
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
        if retry_count > 30 {
            assert_eq!(json, expected, "object_id: {}", object_id);
            break;
          }
        if json == expected {
          break;
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
      }
    }
  }
}

#[allow(dead_code)]
pub async fn get_collab_json_from_server(
  client: &mut client_api::Client,
  workspace_id: &str,
  object_id: &str,
  collab_type: CollabType,
) -> Value {
  let bytes = client
    .get_collab(QueryCollabParams {
      object_id: object_id.to_string(),
      workspace_id: workspace_id.to_string(),
      collab_type,
    })
    .await
    .unwrap();

  Collab::new_with_raw_data(CollabOrigin::Empty, object_id, vec![bytes.to_vec()], vec![])
    .unwrap()
    .to_json_value()
}
