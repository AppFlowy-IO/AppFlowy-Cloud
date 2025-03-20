use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::AFAccessLevel;
use uuid::Uuid;

use crate::{
  act::Action,
  collab::{CollabAccessControl, RealtimeAccessControl},
};

#[derive(Clone)]
pub struct CollabAccessControlImpl;

impl CollabAccessControlImpl {
  pub fn new() -> Self {
    Self {}
  }
}

impl Default for CollabAccessControlImpl {
  fn default() -> Self {
    Self::new()
  }
}

#[async_trait]
impl CollabAccessControl for CollabAccessControlImpl {
  async fn enforce_action(
    &self,
    _workspace_id: &Uuid,
    _uid: &i64,
    _oid: &Uuid,
    _action: Action,
  ) -> Result<(), AppError> {
    Ok(())
  }

  async fn enforce_access_level(
    &self,
    _workspace_id: &Uuid,
    _uid: &i64,
    _oid: &Uuid,
    _access_level: AFAccessLevel,
  ) -> Result<(), AppError> {
    Ok(())
  }

  async fn update_access_level_policy(
    &self,
    _uid: &i64,
    _oid: &Uuid,
    _level: AFAccessLevel,
  ) -> Result<(), AppError> {
    Ok(())
  }

  async fn remove_access_level(&self, _uid: &i64, _oid: &Uuid) -> Result<(), AppError> {
    Ok(())
  }
}

#[derive(Clone)]
pub struct RealtimeCollabAccessControlImpl;

impl RealtimeCollabAccessControlImpl {
  pub fn new() -> Self {
    Self {}
  }
}

impl Default for RealtimeCollabAccessControlImpl {
  fn default() -> Self {
    Self::new()
  }
}

#[async_trait]
impl RealtimeAccessControl for RealtimeCollabAccessControlImpl {
  async fn can_write_collab(
    &self,
    _workspace_id: &Uuid,
    _uid: &i64,
    _oid: &Uuid,
  ) -> Result<bool, AppError> {
    Ok(true)
  }

  async fn can_read_collab(
    &self,
    _workspace_id: &Uuid,
    _uid: &i64,
    _oid: &Uuid,
  ) -> Result<bool, AppError> {
    Ok(true)
  }
}
