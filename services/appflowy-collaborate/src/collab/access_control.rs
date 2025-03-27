use async_trait::async_trait;
use std::sync::Arc;
use uuid::Uuid;

use crate::collab::cache::CollabCache;
use access_control::act::Action;
use access_control::collab::CollabAccessControl;
use access_control::workspace::WorkspaceAccessControl;
use app_error::AppError;
use database::collab::CollabStorageAccessControl;
use database_entity::dto::AFAccessLevel;

#[derive(Clone)]
pub struct CollabStorageAccessControlImpl {
  pub collab_access_control: Arc<dyn CollabAccessControl>,
  pub workspace_access_control: Arc<dyn WorkspaceAccessControl>,
  pub cache: CollabCache,
}

#[async_trait]
impl CollabStorageAccessControl for CollabStorageAccessControlImpl {
  async fn update_policy(
    &self,
    uid: &i64,
    oid: &str,
    level: AFAccessLevel,
  ) -> Result<(), AppError> {
    self
      .collab_access_control
      .update_access_level_policy(uid, oid, level)
      .await
  }

  async fn enforce_read_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
  ) -> Result<(), AppError> {
    let collab_exists = self.cache.is_exist(workspace_id, oid).await?;
    if !collab_exists {
      // If the collab does not exist, we should not enforce the access control. We consider the user
      // has the permission to read the collab
      return Ok(());
    }
    self
      .collab_access_control
      .enforce_action(workspace_id, uid, oid, Action::Read)
      .await
  }

  async fn enforce_write_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
  ) -> Result<(), AppError> {
    let collab_exists = self.cache.is_exist(workspace_id, oid).await?;
    if !collab_exists {
      // If the collab does not exist, we should not enforce the access control. we consider the user
      // has the permission to write the collab
      return Ok(());
    }
    self
      .collab_access_control
      .enforce_action(workspace_id, uid, oid, Action::Write)
      .await
  }

  async fn enforce_write_workspace(&self, uid: &i64, workspace_id: &str) -> Result<(), AppError> {
    let workspace_id = Uuid::parse_str(workspace_id)?;
    self
      .workspace_access_control
      .enforce_action(uid, &workspace_id, Action::Write)
      .await
  }

  async fn enforce_delete(&self, workspace_id: &str, uid: &i64, oid: &str) -> Result<(), AppError> {
    self
      .collab_access_control
      .enforce_access_level(workspace_id, uid, oid, AFAccessLevel::FullAccess)
      .await
  }
}
