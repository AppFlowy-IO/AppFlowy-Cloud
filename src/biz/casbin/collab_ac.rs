use crate::biz::collab::access_control::CollabAccessControl;
use access_control::access::ObjectType;
use access_control::access::{enable_access_control, AccessControl, Action, ActionVariant};
use app_error::AppError;
use async_trait::async_trait;

use collab_rt::RealtimeAccessControl;

use database_entity::dto::AFAccessLevel;

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
