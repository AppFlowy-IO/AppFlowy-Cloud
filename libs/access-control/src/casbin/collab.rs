use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::AFAccessLevel;
use tracing::instrument;

use crate::{
  act::{Action, ActionVariant},
  collab::{CollabAccessControl, RealtimeAccessControl},
  entity::{ObjectType, SubjectType},
};

use super::access::AccessControl;

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
  ) -> Result<(), AppError> {
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
  ) -> Result<(), AppError> {
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
        SubjectType::User(*uid),
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
      .remove_policy(&SubjectType::User(*uid), &ObjectType::Collab(oid))
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

  async fn can_perform_action(
    &self,
    workspace_id: &str,
    uid: &i64,
    oid: &str,
    required_action: Action,
  ) -> Result<bool, AppError> {
    self
      .access_control
      .enforce(
        workspace_id,
        uid,
        ObjectType::Collab(oid),
        ActionVariant::FromAction(&required_action),
      )
      .await?;

    Ok(true)
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
