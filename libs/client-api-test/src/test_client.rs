use crate::{localhost_client_with_device_id, setup_log};
use anyhow::{anyhow, Error};
use assert_json_diff::{
  assert_json_eq, assert_json_include, assert_json_matches_no_panic, CompareMode, Config,
};
use bytes::Bytes;
#[cfg(feature = "collab-sync")]
use client_api::collab_sync::{SinkConfig, SyncObject, SyncPlugin};
use client_api::entity::QueryWorkspaceMember;
use client_api::ws::{WSClient, WSClientConfig};
use collab::core::collab::{DataSource, MutexCollab};
use collab::core::collab_state::SyncState;
use collab::core::origin::{CollabClient, CollabOrigin};
use collab::entity::EncodedCollab;
use collab::preclude::Collab;

use collab_entity::CollabType;
use collab_folder::Folder;
use database_entity::dto::{
  AFAccessLevel, AFRole, AFSnapshotMeta, AFSnapshotMetas, AFUserWorkspaceInfo, AFWorkspace,
  AFWorkspaceInvitationStatus, AFWorkspaceMember, BatchQueryCollabResult, CollabParams,
  CreateCollabParams, InsertCollabMemberParams, QueryCollab, QueryCollabParams,
  QuerySnapshotParams, SnapshotData, UpdateCollabMemberParams,
};
use mime::Mime;
use serde::Deserialize;
use serde_json::{json, Value};
use shared_entity::dto::workspace_dto::{
  BlobMetadata, CollabResponse, WorkspaceMemberChangeset, WorkspaceMemberInvitation,
  WorkspaceSpaceUsage,
};
use shared_entity::response::AppResponseError;
use std::collections::HashMap;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{sleep, timeout, Duration};
use tokio_stream::StreamExt;
use tracing::trace;
use uuid::Uuid;

use crate::user::{generate_unique_registered_user, User};

pub struct TestClient {
  pub user: User,
  pub ws_client: WSClient,
  pub api_client: client_api::Client,
  pub collabs: HashMap<String, TestCollab>,
  pub device_id: String,
}
pub struct TestCollab {
  #[allow(dead_code)]
  pub origin: CollabOrigin,
  pub mutex_collab: Arc<MutexCollab>,
}
impl TestClient {
  pub async fn new(registered_user: User, start_ws_conn: bool) -> Self {
    setup_log();
    let device_id = Uuid::new_v4().to_string();
    Self::new_with_device_id(&device_id, registered_user, start_ws_conn).await
  }

  pub async fn new_with_device_id(
    device_id: &str,
    registered_user: User,
    start_ws_conn: bool,
  ) -> Self {
    setup_log();
    let api_client = localhost_client_with_device_id(device_id);
    api_client
      .sign_in_password(&registered_user.email, &registered_user.password)
      .await
      .unwrap();
    let device_id = api_client.device_id.clone();

    // Connect to server via websocket
    let ws_client = WSClient::new(
      WSClientConfig {
        buffer_capacity: 100,
        ping_per_secs: 6,
        retry_connect_per_pings: 5,
      },
      api_client.clone(),
      api_client.clone(),
    );
    if start_ws_conn {
      ws_client.connect().await.unwrap();
    }
    Self {
      user: registered_user,
      ws_client,
      api_client,
      collabs: Default::default(),
      device_id,
    }
  }

  pub async fn new_user() -> Self {
    let registered_user = generate_unique_registered_user().await;
    let this = Self::new(registered_user, true).await;
    let uid = this.uid().await;
    trace!("🤖New user created: {}", uid);
    this
  }

  pub async fn new_user_without_ws_conn() -> Self {
    let registered_user = generate_unique_registered_user().await;
    Self::new(registered_user, false).await
  }

  pub async fn get_connect_users(&self, object_id: &str) -> Vec<i64> {
    #[derive(Deserialize)]
    struct UserId {
      pub uid: i64,
    }

    self
      .collabs
      .get(object_id)
      .unwrap()
      .mutex_collab
      .lock()
      .get_awareness()
      .clients()
      .iter()
      .map(|(_a, json)| {
        let user: UserId = serde_json::from_str(json).unwrap();
        user.uid
      })
      .collect()
  }

  pub fn clean_awareness_state(&self, object_id: &str) {
    self
      .collabs
      .get(object_id)
      .unwrap()
      .mutex_collab
      .lock()
      .clean_awareness_state();
  }

  pub fn emit_awareness_state(&self, object_id: &str) {
    self
      .collabs
      .get(object_id)
      .unwrap()
      .mutex_collab
      .lock()
      .emit_awareness_state();
  }

  pub async fn user_with_new_device(registered_user: User) -> Self {
    Self::new(registered_user, true).await
  }

  pub async fn get_user_workspace_info(&self) -> AFUserWorkspaceInfo {
    self.api_client.get_user_workspace_info().await.unwrap()
  }

  pub async fn open_workspace(&self, workspace_id: &str) -> AFWorkspace {
    self.api_client.open_workspace(workspace_id).await.unwrap()
  }

  pub async fn get_user_folder(&self) -> Folder {
    let uid = self.uid().await;
    let workspace_id = self.workspace_id().await;
    let data = self
      .api_client
      .get_collab(QueryCollabParams::new(
        &workspace_id,
        CollabType::Folder,
        &workspace_id,
      ))
      .await
      .unwrap();

    Folder::from_collab_doc_state(
      uid,
      CollabOrigin::Empty,
      DataSource::DocStateV1(data.encode_collab.doc_state.to_vec()),
      &workspace_id,
      vec![],
    )
    .unwrap()
  }

  pub async fn try_update_workspace_member(
    &self,
    workspace_id: &str,
    other_client: &TestClient,
    role: AFRole,
  ) -> Result<(), AppResponseError> {
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

  pub async fn invite_and_accepted_workspace_member(
    &self,
    workspace_id: &str,
    other_client: &TestClient,
    role: AFRole,
  ) -> Result<(), AppResponseError> {
    let email = other_client.email().await;

    self
      .api_client
      .invite_workspace_members(
        workspace_id,
        vec![WorkspaceMemberInvitation { email, role }],
      )
      .await?;

    let invitations = other_client
      .api_client
      .list_workspace_invitations(Some(AFWorkspaceInvitationStatus::Pending))
      .await
      .unwrap();

    let target_invitation = invitations
      .iter()
      .find(|inv| inv.workspace_id.to_string().as_str() == workspace_id)
      .unwrap();

    other_client
      .api_client
      .accept_workspace_invitation(target_invitation.invite_id.to_string().as_str())
      .await
      .unwrap();

    Ok(())
  }

  pub async fn try_remove_workspace_member(
    &self,
    workspace_id: &str,
    other_client: &TestClient,
  ) -> Result<(), AppResponseError> {
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

  pub async fn try_get_workspace_members(
    &self,
    workspace_id: &str,
  ) -> Result<Vec<AFWorkspaceMember>, AppResponseError> {
    self.api_client.get_workspace_members(workspace_id).await
  }

  pub async fn get_workspace_member(&self, workspace_id: &str, user_id: i64) -> AFWorkspaceMember {
    let params = QueryWorkspaceMember {
      workspace_id: workspace_id.to_string(),
      uid: user_id,
    };
    self.api_client.get_workspace_member(params).await.unwrap()
  }

  pub async fn try_get_workspace_member(
    &self,
    workspace_id: &str,
    user_id: i64,
  ) -> Result<AFWorkspaceMember, AppResponseError> {
    let params = QueryWorkspaceMember {
      workspace_id: workspace_id.to_string(),
      uid: user_id,
    };

    self.api_client.get_workspace_member(params).await
  }

  pub async fn add_collab_member(
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

  pub async fn update_collab_member_access_level(
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

  pub async fn wait_object_sync_complete(&self, object_id: &str) -> Result<(), Error> {
    self
      .wait_object_sync_complete_with_secs(object_id, 60)
      .await
  }

  pub async fn wait_object_sync_complete_with_secs(
    &self,
    object_id: &str,
    secs: u64,
  ) -> Result<(), Error> {
    let mut sync_state = self
      .collabs
      .get(object_id)
      .unwrap()
      .mutex_collab
      .lock()
      .subscribe_sync_state();

    let duration = Duration::from_secs(secs);
    while let Ok(Some(state)) = timeout(duration, sync_state.next()).await {
      if state == SyncState::SyncFinished {
        return Ok(());
      }
    }

    Err(anyhow!(
      "Timeout or SyncState stream ended before reaching SyncFinished"
    ))
  }

  #[allow(dead_code)]
  pub async fn get_blob_metadata(&self, workspace_id: &str, file_id: &str) -> BlobMetadata {
    let url = self.api_client.get_blob_url(workspace_id, file_id);
    self.api_client.get_blob_metadata(&url).await.unwrap()
  }

  pub async fn upload_blob<T: Into<Bytes>>(&self, file_id: &str, data: T, mime: &Mime) {
    let workspace_id = self.workspace_id().await;
    let url = self.api_client.get_blob_url(&workspace_id, file_id);
    self.api_client.put_blob(&url, data, mime).await.unwrap()
  }

  pub async fn delete_file(&self, file_id: &str) {
    let workspace_id = self.workspace_id().await;
    let url = self.api_client.get_blob_url(&workspace_id, file_id);
    self.api_client.delete_blob(&url).await.unwrap();
  }

  pub async fn get_workspace_usage(&self) -> WorkspaceSpaceUsage {
    let workspace_id = self.workspace_id().await;
    self
      .api_client
      .get_workspace_usage(&workspace_id)
      .await
      .unwrap()
  }

  pub async fn workspace_id(&self) -> String {
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

  pub async fn email(&self) -> String {
    self.api_client.get_profile().await.unwrap().email.unwrap()
  }

  pub async fn uid(&self) -> i64 {
    self.api_client.get_profile().await.unwrap().uid
  }

  pub async fn get_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    snapshot_id: &i64,
  ) -> Result<SnapshotData, AppResponseError> {
    self
      .api_client
      .get_snapshot(
        workspace_id,
        object_id,
        QuerySnapshotParams {
          snapshot_id: *snapshot_id,
        },
      )
      .await
  }

  pub async fn create_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Result<AFSnapshotMeta, AppResponseError> {
    self
      .api_client
      .create_snapshot(workspace_id, object_id, collab_type)
      .await
  }

  pub async fn get_snapshot_list(
    &self,
    workspace_id: &str,
    object_id: &str,
  ) -> Result<AFSnapshotMetas, AppResponseError> {
    self
      .api_client
      .get_snapshot_list(workspace_id, object_id)
      .await
  }

  pub async fn get_snapshot_list_until(
    &self,
    workspace_id: &str,
    object_id: &str,
    f: impl Fn(&AFSnapshotMetas) -> bool + Send + Sync + 'static,
    timeout_secs: u64,
  ) -> Result<AFSnapshotMetas, AppResponseError> {
    let duration = Duration::from_secs(timeout_secs);
    #[allow(clippy::blocks_in_conditions)]
    match timeout(duration, async {
      let mut snapshot_metas = self.get_snapshot_list(workspace_id, object_id).await?;
      // Loop until the condition `f` returns true or the timeout is reached
      while !f(&snapshot_metas) {
        sleep(Duration::from_secs(5)).await;
        snapshot_metas = self.get_snapshot_list(workspace_id, object_id).await?;
      }
      Ok(snapshot_metas)
    })
    .await
    {
      Ok(result) => result,
      Err(_) => panic!("Operation timed out after {} seconds", timeout_secs),
    }
  }

  pub async fn create_collab_list(
    &mut self,
    workspace_id: &str,
    params: Vec<CollabParams>,
  ) -> Result<(), AppResponseError> {
    self
      .api_client
      .create_collab_list(workspace_id, params)
      .await
  }

  pub async fn get_collab(
    &mut self,
    query: QueryCollabParams,
  ) -> Result<CollabResponse, AppResponseError> {
    self.api_client.get_collab(query).await
  }

  pub async fn batch_get_collab(
    &mut self,
    workspace_id: &str,
    params: Vec<QueryCollab>,
  ) -> Result<BatchQueryCollabResult, AppResponseError> {
    self.api_client.batch_get_collab(workspace_id, params).await
  }

  #[allow(clippy::await_holding_lock)]
  pub async fn create_and_edit_collab(
    &mut self,
    workspace_id: &str,
    collab_type: CollabType,
  ) -> String {
    let object_id = Uuid::new_v4().to_string();
    self
      .create_and_edit_collab_with_data(&object_id, workspace_id, collab_type, None)
      .await;
    object_id
  }

  #[allow(unused_variables)]
  pub async fn create_and_edit_collab_with_data(
    &mut self,
    object_id: &str,
    workspace_id: &str,
    collab_type: CollabType,
    encoded_collab_v1: Option<EncodedCollab>,
  ) {
    // Subscribe to object
    let origin = CollabOrigin::Client(CollabClient::new(self.uid().await, self.device_id.clone()));
    let collab = match encoded_collab_v1 {
      None => Arc::new(MutexCollab::new(Collab::new_with_origin(
        origin.clone(),
        object_id,
        vec![],
        false,
      ))),
      Some(data) => Arc::new(MutexCollab::new(
        Collab::new_with_source(
          origin.clone(),
          object_id,
          DataSource::DocStateV1(data.doc_state.to_vec()),
          vec![],
          false,
        )
        .unwrap(),
      )),
    };

    let encoded_collab_v1 = {
      let mut lock_guard = collab.lock();
      lock_guard.emit_awareness_state();
      let data = lock_guard
        .encode_collab_v1(|collab| collab_type.validate_require_data(collab))
        .unwrap()
        .encode_to_bytes()
        .unwrap();
      drop(lock_guard);
      data
    };

    self
      .api_client
      .create_collab(CreateCollabParams {
        object_id: object_id.to_string(),
        encoded_collab_v1,
        collab_type: collab_type.clone(),
        workspace_id: workspace_id.to_string(),
      })
      .await
      .unwrap();

    #[cfg(feature = "collab-sync")]
    {
      let handler = self
        .ws_client
        .subscribe_collab(object_id.to_string())
        .unwrap();
      let (sink, stream) = (handler.sink(), handler.stream());
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
      collab.lock().add_plugin(Box::new(sync_plugin));
    }
    collab.lock().initialize();
    let test_collab = TestCollab {
      origin,
      mutex_collab: collab,
    };
    self.collabs.insert(object_id.to_string(), test_collab);
    self.wait_object_sync_complete(object_id).await.unwrap();
  }

  pub async fn open_workspace_collab(&mut self, workspace_id: &str) {
    self
      .open_collab(workspace_id, workspace_id, CollabType::Unknown)
      .await;
  }

  #[allow(clippy::await_holding_lock)]
  pub async fn open_collab(
    &mut self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) {
    self
      .open_collab_with_doc_state(workspace_id, object_id, collab_type, vec![])
      .await
  }

  #[allow(unused_variables)]
  pub async fn open_collab_with_doc_state(
    &mut self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
    doc_state: Vec<u8>,
  ) {
    // Subscribe to object
    let origin = CollabOrigin::Client(CollabClient::new(self.uid().await, self.device_id.clone()));
    let collab = Arc::new(MutexCollab::new(
      Collab::new_with_source(
        origin.clone(),
        object_id,
        DataSource::DocStateV1(doc_state),
        vec![],
        false,
      )
      .unwrap(),
    ));
    collab.lock().emit_awareness_state();

    #[cfg(feature = "collab-sync")]
    {
      let handler = self
        .ws_client
        .subscribe_collab(object_id.to_string())
        .unwrap();
      let (sink, stream) = (handler.sink(), handler.stream());
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

      collab.lock().add_plugin(Box::new(sync_plugin));
    }
    collab.lock().initialize();
    let test_collab = TestCollab {
      origin,
      mutex_collab: collab,
    };
    self.collabs.insert(object_id.to_string(), test_collab);
  }

  #[allow(unused_variables)]
  pub async fn create_collab_with_data(
    &mut self,
    object_id: String,
    workspace_id: &str,
    collab_type: CollabType,
    encoded_collab_v1: Option<EncodedCollab>,
  ) -> Result<(), AppResponseError> {
    // Subscribe to object
    let origin = CollabOrigin::Client(CollabClient::new(self.uid().await, self.device_id.clone()));
    let collab = match encoded_collab_v1 {
      None => Arc::new(MutexCollab::new(Collab::new_with_origin(
        origin.clone(),
        &object_id,
        vec![],
        false,
      ))),
      Some(data) => Arc::new(MutexCollab::new(
        Collab::new_with_source(
          origin.clone(),
          &object_id,
          DataSource::DocStateV1(data.doc_state.to_vec()),
          vec![],
          false,
        )
        .unwrap(),
      )),
    };

    let encoded_collab_v1 = {
      let lock_guard = collab.lock();
      let data = lock_guard
        .encode_collab_v1(|collab| collab_type.validate_require_data(collab))
        .unwrap()
        .encode_to_bytes()
        .unwrap();
      drop(lock_guard);
      data
    };

    self
      .api_client
      .create_collab(CreateCollabParams {
        object_id: object_id.clone(),
        encoded_collab_v1,
        collab_type: collab_type.clone(),
        workspace_id: workspace_id.to_string(),
      })
      .await
  }

  #[cfg(not(target_arch = "wasm32"))]
  pub async fn post_realtime_binary(&self, message: Vec<u8>) -> Result<(), AppResponseError> {
    let message = client_websocket::Message::binary(message);
    self
      .api_client
      .post_realtime_msg(&self.device_id, message)
      .await
  }

  pub async fn disconnect(&self) {
    self.ws_client.disconnect().await;
  }

  pub async fn reconnect(&self) {
    self.ws_client.connect().await.unwrap();
  }

  pub async fn get_edit_collab_json(&self, object_id: &str) -> Value {
    self
      .collabs
      .get(object_id)
      .unwrap()
      .mutex_collab
      .lock()
      .to_json_value()
  }
}

pub async fn assert_server_snapshot(
  client: &client_api::Client,
  workspace_id: &str,
  object_id: &str,
  snapshot_id: &i64,
  expected: Value,
) {
  let workspace_id = workspace_id.to_string();
  let object_id = object_id.to_string();
  let mut retry_count = 0;
  loop {
    tokio::select! {
       _ = tokio::time::sleep(Duration::from_secs(10)) => {
         panic!("Query snapshot timeout");
       },
       result = client.get_snapshot(&workspace_id, &object_id, QuerySnapshotParams {snapshot_id: *snapshot_id },
        ) => {
        retry_count += 1;
        match &result {
          Ok(snapshot_data) => {
          let encoded_collab_v1 =
            EncodedCollab::decode_from_bytes(&snapshot_data.encoded_collab_v1).unwrap();
          let json = Collab::new_with_source(
            CollabOrigin::Empty,
            &object_id,
            DataSource::DocStateV1(encoded_collab_v1.doc_state.to_vec()),
            vec![],
            false,
          )
          .unwrap()
          .to_json_value();
            if retry_count > 10 {
              assert_json_eq!(json, expected);
              break;
            }

            if assert_json_matches_no_panic(&json, &expected, Config::new(CompareMode::Inclusive)).is_ok() {
              break;
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
          },
          Err(e) => {
            if retry_count > 10 {
              panic!("Query snapshot failed: {}", e);
            }
            tokio::time::sleep(Duration::from_millis(1000)).await;
          }
        }
       },
    }
  }
}

pub async fn assert_server_collab(
  workspace_id: &str,
  client: &mut client_api::Client,
  object_id: &str,
  collab_type: &CollabType,
  timeout_secs: u64,
  expected: Value,
) -> Result<(), Error> {
  let duration = Duration::from_secs(timeout_secs);
  let collab_type = collab_type.clone();
  let object_id = object_id.to_string();
  let final_json = Arc::new(Mutex::new(json!({})));

  // Use tokio::time::timeout to apply a timeout to the entire operation
  let cloned_final_json = final_json.clone();
  let operation = async {
    loop {
      let result = client
        .get_collab(QueryCollabParams::new(
          &object_id,
          collab_type.clone(),
          workspace_id,
        ))
        .await;

      match &result {
        Ok(data) => {
          let json = Collab::new_with_source(
            CollabOrigin::Empty,
            &object_id,
            DataSource::DocStateV1(data.encode_collab.doc_state.to_vec()),
            vec![],
            false,
          )
          .unwrap()
          .to_json_value();

          *cloned_final_json.lock().await = json.clone();
          if assert_json_matches_no_panic(&json, &expected, Config::new(CompareMode::Inclusive))
            .is_ok()
          {
            return;
          }
        },
        Err(e) => {
          // Instead of panicking immediately, log or handle the error and continue the loop
          // until the timeout is reached.
          eprintln!("Query collab failed: {}", e);
        },
      }

      // Sleep before retrying. Adjust the sleep duration as needed.
      tokio::time::sleep(Duration::from_millis(1000)).await;
    }
  };

  if timeout(duration, operation).await.is_err() {
    eprintln!("json : {}, expected: {}", final_json.lock().await, expected);
    return Err(anyhow!("time out for the action"));
  }
  Ok(())
}

pub async fn assert_client_collab_within_secs(
  client: &mut TestClient,
  object_id: &str,
  key: &str,
  expected: Value,
  secs: u64,
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
          .collabs
          .get_mut(&object_id)
          .unwrap()
          .mutex_collab
          .lock()
          .to_json_value()
      } => {
        retry_count += 1;
        if retry_count > 60 {
            assert_eq!(json[key], expected[key], "object_id: {}", object_id);
            break;
          }
        if json[key] == expected[key] {
          break;
        }
        tokio::time::sleep(Duration::from_millis(1000)).await;
      }
    }
  }
}

pub async fn assert_client_collab_include_value(
  client: &mut TestClient,
  object_id: &str,
  expected: Value,
) -> Result<(), Error> {
  let secs = 60;
  let object_id = object_id.to_string();
  let mut retry_count = 0;
  loop {
    tokio::select! {
       _ = tokio::time::sleep(Duration::from_secs(secs)) => {
        return Err(anyhow!("timeout"));
       },
       json = async {
        client
          .collabs
          .get_mut(&object_id)
          .unwrap()
          .mutex_collab
          .lock()
          .to_json_value()
      } => {
        retry_count += 1;
        if retry_count > 30 {
          assert_json_include!(actual: json, expected: expected);
          return Ok(());
          }
        if assert_json_matches_no_panic(&json, &expected, Config::new(CompareMode::Inclusive)).is_ok() {
          return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
      }
    }
  }
}

#[allow(dead_code)]
pub async fn get_collab_json_from_server(
  client: &client_api::Client,
  workspace_id: &str,
  object_id: &str,
  collab_type: CollabType,
) -> Value {
  let bytes = client
    .get_collab(QueryCollabParams::new(object_id, collab_type, workspace_id))
    .await
    .unwrap();

  Collab::new_with_source(
    CollabOrigin::Empty,
    object_id,
    DataSource::DocStateV1(bytes.encode_collab.doc_state.to_vec()),
    vec![],
    false,
  )
  .unwrap()
  .to_json_value()
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
