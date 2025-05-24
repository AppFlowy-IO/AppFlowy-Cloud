use anyhow::{anyhow, Error};
use dashmap::DashMap;
use std::sync::Arc;
use uuid::Uuid;

use client_api::v2::{WorkspaceController, WorkspaceControllerOptions, WorkspaceId};
use tempfile::TempDir;

use crate::LOCALHOST_WS_V2;

/// Handles workspace-related operations for the test client
pub struct WorkspaceManager {
  pub workspaces: DashMap<WorkspaceId, Arc<WorkspaceController>>,
  device_id: String,
  temp_dir: Arc<TempDir>,
}

impl WorkspaceManager {
  pub fn new(device_id: String, temp_dir: Arc<TempDir>) -> Self {
    Self {
      workspaces: DashMap::new(),
      device_id,
      temp_dir,
    }
  }

  /// Gets or creates a workspace controller for the given workspace ID
  pub async fn get_or_create_workspace(
    &self,
    workspace_id: Uuid,
    uid: i64,
    access_token: Option<String>,
  ) -> Result<Arc<WorkspaceController>, Error> {
    match self.workspaces.entry(workspace_id) {
      dashmap::mapref::entry::Entry::Occupied(e) => Ok(e.get().clone()),
      dashmap::mapref::entry::Entry::Vacant(e) => {
        let workspace = self.create_workspace_controller(workspace_id, uid).await?;

        if let Some(token) = access_token {
          workspace.connect(token).await?;
        }

        Ok(e.insert(Arc::new(workspace)).value().clone())
      },
    }
  }

  /// Creates a new workspace controller without connecting
  async fn create_workspace_controller(
    &self,
    workspace_id: Uuid,
    uid: i64,
  ) -> Result<WorkspaceController, Error> {
    let db_path = self.get_workspace_db_path(workspace_id).await?;

    let workspace = WorkspaceController::new(
      WorkspaceControllerOptions {
        url: LOCALHOST_WS_V2.to_string(),
        workspace_id,
        uid,
        device_id: self.device_id.clone(),
        sync_eagerly: true,
      },
      &db_path,
    )?;

    Ok(workspace)
  }

  /// Gets the database path for a workspace
  async fn get_workspace_db_path(&self, workspace_id: Uuid) -> Result<String, Error> {
    let db_path = self
      .temp_dir
      .path()
      .to_str()
      .ok_or_else(|| anyhow!("Invalid temp directory path"))?;
    let db_path = format!("{}/{}/{}", db_path, self.device_id, workspace_id);
    tokio::fs::create_dir_all(&db_path).await?;
    Ok(db_path)
  }

  /// Connects all workspaces
  pub async fn connect_all(&self, access_token: String) -> Result<(), Error> {
    let mut errors = Vec::new();

    for entry in self.workspaces.iter() {
      if let Err(e) = entry.value().connect(access_token.clone()).await {
        errors.push(e);
      }
    }

    if !errors.is_empty() {
      return Err(anyhow!("Failed to connect some workspaces: {:?}", errors));
    }

    Ok(())
  }

  /// Disconnects all workspaces
  pub async fn disconnect_all(&self) -> Result<(), Error> {
    let mut errors = Vec::new();

    for entry in self.workspaces.iter() {
      if let Err(e) = entry.value().disconnect().await {
        errors.push(e);
      }
    }

    if !errors.is_empty() {
      return Err(anyhow!(
        "Failed to disconnect some workspaces: {:?}",
        errors
      ));
    }

    Ok(())
  }

  /// Gets a workspace by ID
  pub fn get_workspace(&self, workspace_id: &Uuid) -> Option<Arc<WorkspaceController>> {
    self
      .workspaces
      .get(workspace_id)
      .map(|entry| entry.value().clone())
  }

  /// Enables/disables message receiving for all workspaces (debug builds only)
  #[cfg(debug_assertions)]
  pub fn set_receive_message(&self, enabled: bool) {
    for mut entry in self.workspaces.iter_mut() {
      let workspace = entry.value_mut();
      if enabled {
        workspace.enable_receive_message();
      } else {
        workspace.disable_receive_message();
      }
    }
  }
}
