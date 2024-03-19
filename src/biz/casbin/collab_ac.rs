use crate::biz::casbin::access_control::ObjectType;
use crate::biz::casbin::access_control::{
  enable_access_control, AccessControl, AccessControlChange, Action, ActionVariant,
};
use crate::biz::collab::access_control::CollabAccessControl;
use app_error::AppError;
use async_trait::async_trait;

use dashmap::DashMap;
use database_entity::dto::AFAccessLevel;
use realtime::server::RealtimeAccessControl;
use std::sync::Arc;
use tracing::instrument;

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
  action_by_oid: Arc<DashMap<String, Action>>,
}

impl RealtimeCollabAccessControlImpl {
  pub fn new(access_control: AccessControl) -> Self {
    let action_by_oid = Arc::new(DashMap::new());
    let mut sub = access_control.subscribe_change();
    let weak_action_by_oid = Arc::downgrade(&action_by_oid);

    tokio::spawn(async move {
      while let Ok(change) = sub.recv().await {
        match weak_action_by_oid.upgrade() {
          None => break,
          Some(action_by_oid) => match change {
            AccessControlChange::UpdatePolicy { uid, oid } => {
              action_by_oid.remove(&cache_key(uid, &oid));
            },
            AccessControlChange::RemovePolicy { uid, oid } => {
              action_by_oid.remove(&cache_key(uid, &oid));
            },
          },
        }
      }
    });

    Self {
      access_control,
      action_by_oid,
    }
  }

  async fn can_perform_action(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
    required_action: Action,
  ) -> Result<bool, AppError> {
    if enable_access_control() {
      let key = cache_key(*uid, oid);
      // Check if the action is already cached
      if let Some(action) = self.action_by_oid.get(&key) {
        return Ok(*action >= required_action);
      }

      // Not in cache, enforce access control
      let is_permitted = self
        .access_control
        .enforce(
          workspace_id,
          uid,
          ObjectType::Collab(oid),
          ActionVariant::FromAction(&required_action),
        )
        .await?;

      if is_permitted {
        // Permission granted, cache the action
        self.action_by_oid.insert(key, required_action);
      }

      Ok(is_permitted)
    } else {
      Ok(true)
    }
  }
}

#[inline]
fn cache_key(uid: i64, oid: &str) -> String {
  format!("{}:{}", uid, oid)
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
