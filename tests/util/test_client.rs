use assert_json_diff::assert_json_eq;
use bytes::Bytes;
use client_api::collab_sync::{SinkConfig, SyncObject, SyncPlugin};
use client_api::ws::{BusinessID, WSClient, WSClientConfig};
use collab::core::collab::MutexCollab;
use collab::core::collab_state::SyncState;
use collab::core::origin::{CollabClient, CollabOrigin};
use collab::preclude::Collab;
use collab_entity::CollabType;
use database_entity::dto::{
  AFAccessLevel, AFBlobMetadata, AFRole, AFUserWorkspaceInfo, AFWorkspace, AFWorkspaceMember,
  InsertCollabMemberParams, QueryCollabParams, UpdateCollabMemberParams,
};
use image::io::Reader as ImageReader;
use serde_json::Value;
use shared_entity::app_error::AppError;
use shared_entity::dto::workspace_dto::{
  CreateWorkspaceMember, WorkspaceMemberChangeset, WorkspaceSpaceUsage,
};
use sqlx::types::Uuid;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::tempdir;
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
  pub(crate) async fn new(device_id: String, registered_user: User, invoke_ws_conn: bool) -> Self {
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

    if invoke_ws_conn {
      ws_client
        .connect(api_client.ws_url(&device_id).unwrap())
        .await
        .unwrap();
    }
    Self {
      ws_client,
      api_client,
      collab_by_object_id: Default::default(),
      device_id,
    }
  }

  pub(crate) async fn new_user() -> Self {
    let registered_user = generate_unique_registered_user().await;
    let device_id = Uuid::new_v4().to_string();
    Self::new(device_id, registered_user, true).await
  }

  pub(crate) async fn new_user_without_ws_conn() -> Self {
    let registered_user = generate_unique_registered_user().await;
    let device_id = Uuid::new_v4().to_string();
    Self::new(device_id, registered_user, false).await
  }

  pub(crate) async fn user_with_new_device(registered_user: User) -> Self {
    let device_id = Uuid::new_v4().to_string();
    Self::new(device_id, registered_user, true).await
  }

  pub(crate) async fn add_workspace_member(
    &self,
    workspace_id: &str,
    other_client: &TestClient,
    role: AFRole,
  ) {
    self
      .try_add_workspace_member(workspace_id, other_client, role)
      .await
      .unwrap();
  }

  pub(crate) async fn get_user_workspace_info(&self) -> AFUserWorkspaceInfo {
    self.api_client.get_user_workspace_info().await.unwrap()
  }

  pub(crate) async fn open_workspace(&self, workspace_id: &str) -> AFWorkspace {
    self.api_client.open_workspace(workspace_id).await.unwrap()
  }

  pub(crate) async fn try_update_workspace_member(
    &self,
    workspace_id: &str,
    other_client: &TestClient,
    role: AFRole,
  ) -> Result<(), AppError> {
    let workspace_id = Uuid::parse_str(workspace_id).unwrap().to_string();
    let email = other_client.email().await;
    self
      .api_client
      .update_workspace_member(
        workspace_id,
        WorkspaceMemberChangeset::new(email).with_role(role),
      )
      .await
  }

  pub(crate) async fn try_add_workspace_member(
    &self,
    workspace_id: &str,
    other_client: &TestClient,
    role: AFRole,
  ) -> Result<(), AppError> {
    let email = other_client.email().await;
    self
      .api_client
      .add_workspace_members(workspace_id, vec![CreateWorkspaceMember { email, role }])
      .await
  }

  pub(crate) async fn try_remove_workspace_member(
    &self,
    workspace_id: &str,
    other_client: &TestClient,
  ) -> Result<(), AppError> {
    let email = other_client.email().await;
    self
      .api_client
      .remove_workspace_members(workspace_id.to_string(), vec![email])
      .await
  }

  pub async fn get_workspace_members(&self, workspace_id: &str) -> Vec<AFWorkspaceMember> {
    self
      .api_client
      .get_workspace_members(workspace_id)
      .await
      .unwrap()
  }

  pub(crate) async fn add_client_as_collab_member(
    &self,
    workspace_id: &str,
    object_id: &str,
    other_client: &TestClient,
    access_level: AFAccessLevel,
  ) {
    let uid = other_client.uid().await;
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
    let uid = other_client.uid().await;
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

  pub async fn download_blob<T: AsRef<str>>(&self, url: T) -> Vec<u8> {
    self.api_client.get_blob(url).await.unwrap().to_vec()
  }

  #[allow(dead_code)]
  pub async fn get_blob_metadata<T: AsRef<str>>(&self, url: T) -> AFBlobMetadata {
    self.api_client.get_blob_metadata(url).await.unwrap()
  }

  pub async fn upload_blob<T: Into<Bytes>, M: ToString>(&self, data: T, mime: M) -> String {
    let workspace_id = self.workspace_id().await;
    self
      .api_client
      .put_blob(&workspace_id, data, mime)
      .await
      .unwrap()
  }

  pub async fn upload_file_with_path(&self, path: &str) -> String {
    let workspace_id = self.workspace_id().await;
    self
      .api_client
      .put_blob_with_path(&workspace_id, path)
      .await
      .unwrap()
  }

  pub async fn delete_file(&self, url: &str) {
    self.api_client.delete_blob(url).await.unwrap();
  }

  pub async fn get_workspace_usage(&self) -> WorkspaceSpaceUsage {
    let workspace_id = self.workspace_id().await;
    self
      .api_client
      .get_workspace_usage(&workspace_id)
      .await
      .unwrap()
  }

  pub(crate) async fn workspace_id(&self) -> String {
    self
      .api_client
      .get_workspaces()
      .await
      .unwrap()
      .0
      .first()
      .unwrap()
      .workspace_id
      .to_string()
  }

  pub(crate) async fn email(&self) -> String {
    self.api_client.get_profile().await.unwrap().email.unwrap()
  }

  pub(crate) async fn uid(&self) -> i64 {
    self.api_client.get_profile().await.unwrap().uid
  }

  #[allow(clippy::await_holding_lock)]
  pub(crate) async fn create_collab(
    &mut self,
    workspace_id: &str,
    collab_type: CollabType,
  ) -> String {
    let object_id = Uuid::new_v4().to_string();
    // Subscribe to object
    let handler = self
      .ws_client
      .subscribe(BusinessID::CollabId, object_id.clone())
      .unwrap();
    let (sink, stream) = (handler.sink(), handler.stream());
    let origin = CollabOrigin::Client(CollabClient::new(self.uid().await, self.device_id.clone()));
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

  pub(crate) async fn edit_workspace_collab(&mut self, workspace_id: &str) {
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
    // Subscribe to object
    let handler = self
      .ws_client
      .subscribe(BusinessID::CollabId, object_id.to_string())
      .unwrap();
    let (sink, stream) = (handler.sink(), handler.stream());
    let origin = CollabOrigin::Client(CollabClient::new(self.uid().await, self.device_id.clone()));
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

pub fn generate_temp_file_path<T: AsRef<Path>>(file_name: T) -> TestTempFile {
  let mut path = tempdir().unwrap().into_path();
  path.push(file_name);
  TestTempFile(path)
}

pub struct TestTempFile(PathBuf);

impl TestTempFile {
  fn cleanup(dir: &PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
  }
}

impl AsRef<Path> for TestTempFile {
  fn as_ref(&self) -> &Path {
    &self.0
  }
}

impl Deref for TestTempFile {
  type Target = PathBuf;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl Drop for TestTempFile {
  fn drop(&mut self) {
    Self::cleanup(&self.0)
  }
}

pub fn assert_image_equal<P: AsRef<Path>>(path1: P, path2: P) {
  let img1 = ImageReader::open(path1)
    .unwrap()
    .decode()
    .unwrap()
    .into_rgba8();
  let img2 = ImageReader::open(path2)
    .unwrap()
    .decode()
    .unwrap()
    .into_rgba8();

  assert_eq!(img1, img2)
}
