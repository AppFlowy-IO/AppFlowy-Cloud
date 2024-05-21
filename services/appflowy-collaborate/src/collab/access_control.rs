use async_trait::async_trait;
use std::sync::Arc;
use tracing::instrument;

use crate::collab::cache::CollabCache;
use access_control::access::ObjectType;
use access_control::access::{enable_access_control, AccessControl};
use access_control::act::{Action, ActionVariant};
use access_control::collab::{CollabAccessControl, RealtimeAccessControl};
use access_control::workspace::WorkspaceAccessControl;
use app_error::AppError;
use database::collab::CollabStorageAccessControl;
use database_entity::dto::AFAccessLevel;

#[derive(Clone)]
pub struct CollabAccessControlImpl {
  access_control: AccessControl,
}

impl CollabAccessControlImpl {
  pub fn new(access_control: AccessControl) -> Self {
    Self { access_control }
  }
}

#[async_trait]
impl CollabAccessControl for CollabAccessControlImpl {
  async fn enforce_action(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
    action: Action,
  ) -> Result<bool, AppError> {
    self
      .access_control
      .enforce(
        workspace_id,
        uid,
        ObjectType::Collab(oid),
        ActionVariant::FromAction(&action),
      )
      .await
  }

  async fn enforce_access_level(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
    access_level: AFAccessLevel,
  ) -> Result<bool, AppError> {
    self
      .access_control
      .enforce(
        workspace_id,
        uid,
        ObjectType::Collab(oid),
        ActionVariant::FromAccessLevel(&access_level),
      )
      .await
  }

  #[instrument(level = "info", skip_all)]
  async fn update_access_level_policy(
    &self,
    uid: &i64,
    oid: &str,
    level: AFAccessLevel,
  ) -> Result<(), AppError> {
    self
      .access_control
      .update_policy(
        uid,
        ObjectType::Collab(oid),
        ActionVariant::FromAccessLevel(&level),
      )
      .await?;

    Ok(())
  }

  #[instrument(level = "info", skip_all)]
  async fn remove_access_level(&self, uid: &i64, oid: &str) -> Result<(), AppError> {
    self
      .access_control
      .remove_policy(uid, &ObjectType::Collab(oid))
      .await?;
    Ok(())
  }
}

#[derive(Clone)]
pub struct RealtimeCollabAccessControlImpl {
  access_control: AccessControl,
}

impl RealtimeCollabAccessControlImpl {
  pub fn new(access_control: AccessControl) -> Self {
    // let action_by_oid = Arc::new(DashMap::new());
    // let mut sub = access_control.subscribe_change();
    // let weak_action_by_oid = Arc::downgrade(&action_by_oid);
    // tokio::spawn(async move {
    //   while let Ok(change) = sub.recv().await {
    //     match weak_action_by_oid.upgrade() {
    //       None => break,
    //       Some(action_by_oid) => match change {
    //         AccessControlChange::UpdatePolicy { uid, oid } => {},
    //         AccessControlChange::RemovePolicy { uid, oid } => {},
    //       },
    //     }
    //   }
    // });
    Self { access_control }
  }

  async fn can_perform_action(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
    required_action: Action,
  ) -> Result<bool, AppError> {
    if enable_access_control() {
      let is_permitted = self
        .access_control
        .enforce(
          workspace_id,
          uid,
          ObjectType::Collab(oid),
          ActionVariant::FromAction(&required_action),
        )
        .await?;

      Ok(is_permitted)
    } else {
      Ok(true)
    }
  }
}

#[async_trait]
impl RealtimeAccessControl for RealtimeCollabAccessControlImpl {
  async fn can_write_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
  ) -> Result<bool, AppError> {
    self
      .can_perform_action(workspace_id, uid, oid, Action::Write)
      .await
  }

  async fn can_read_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
  ) -> Result<bool, AppError> {
    self
      .can_perform_action(workspace_id, uid, oid, Action::Read)
      .await
  }
}

#[derive(Clone)]
pub struct CollabStorageAccessControlImpl<CollabAC, WorkspaceAC> {
  pub collab_access_control: Arc<CollabAC>,
  pub workspace_access_control: Arc<WorkspaceAC>,
  pub cache: CollabCache,
}

#[async_trait]
impl<CollabAC, WorkspaceAC> CollabStorageAccessControl
  for CollabStorageAccessControlImpl<CollabAC, WorkspaceAC>
where
  CollabAC: CollabAccessControl,
  WorkspaceAC: WorkspaceAccessControl,
{
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
  ) -> Result<bool, AppError> {
    let collab_exists = self.cache.is_exist(oid).await?;
    if !collab_exists {
      // If the collab does not exist, we should not enforce the access control. We consider the user
      // has the permission to read the collab
      return Ok(true);
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
  ) -> Result<bool, AppError> {
    let collab_exists = self.cache.is_exist(oid).await?;
    if !collab_exists {
      // If the collab does not exist, we should not enforce the access control. we consider the user
      // has the permission to write the collab
      return Ok(true);
    }
    self
      .collab_access_control
      .enforce_action(workspace_id, uid, oid, Action::Write)
      .await
  }

  async fn enforce_write_workspace(&self, uid: &i64, workspace_id: &str) -> Result<bool, AppError> {
    self
      .workspace_access_control
      .enforce_action(uid, workspace_id, Action::Write)
      .await
  }

  async fn enforce_delete(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
  ) -> Result<bool, AppError> {
    self
      .collab_access_control
      .enforce_access_level(workspace_id, uid, oid, AFAccessLevel::FullAccess)
      .await
  }
}
