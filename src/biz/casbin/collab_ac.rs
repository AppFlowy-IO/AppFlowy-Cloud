use crate::biz::casbin::access_control::{AccessControl, Action};
use crate::biz::casbin::access_control::{ActionType, ObjectType};
use crate::biz::collab::access_control::CollabAccessControl;
use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::AFAccessLevel;
use realtime::collaborate::RealtimeAccessControl;
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
  async fn enforce_action(&self, uid: &i64, oid: &str, action: Action) -> Result<bool, AppError> {
    self
      .access_control
      .enforce(uid, &ObjectType::Collab(oid), action)
      .await
  }

  async fn enforce_access_level(
    &self,
    uid: &i64,
    oid: &str,
    access_level: AFAccessLevel,
  ) -> Result<bool, AppError> {
    self
      .access_control
      .enforce(uid, &ObjectType::Collab(oid), access_level)
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
      .update_policy(uid, &ObjectType::Collab(oid), &ActionType::Level(level))
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
    Self { access_control }
  }
}

#[async_trait]
impl RealtimeAccessControl for RealtimeCollabAccessControlImpl {
  async fn can_send_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    if cfg!(feature = "disable_access_control") {
      Ok(true)
    } else {
      self
        .access_control
        .enforce(uid, &ObjectType::Collab(oid), Action::Write)
        .await
    }
  }

  async fn can_receive_collab_update(&self, uid: &i64, oid: &str) -> Result<bool, AppError> {
    if cfg!(feature = "disable_access_control") {
      Ok(true)
    } else {
      self
        .access_control
        .enforce(uid, &ObjectType::Collab(oid), Action::Read)
        .await
    }
  }
}
