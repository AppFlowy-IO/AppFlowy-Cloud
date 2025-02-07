use app_error::AppError;
use async_trait::async_trait;
use database_entity::dto::AFAccessLevel;
use tracing::instrument;

use crate::{
  act::Action,
  collab::{CollabAccessControl, RealtimeAccessControl},
  entity::ObjectType,
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
    _oid: &str,
    action: Action,
  ) -> Result<(), AppError> {
    // TODO: allow non workspace member to read a collab.

    // Anyone who can write to a workspace, can also delete a collab.
    let workspace_action = match action {
      Action::Read => Action::Read,
      Action::Write => Action::Write,
      Action::Delete => Action::Write,
    };

    let result = self
      .access_control
      .enforce(
        uid,
        ObjectType::Workspace(workspace_id.to_string()),
        workspace_action,
      )
      .await;
    match result {
      Ok(true) => Ok(()),
      Ok(false) => Err(AppError::NotEnoughPermissions),
      Err(e) => Err(e),
    }
  }

  async fn enforce_access_level(
    &self,
    workspace_id: &str,
    uid: &i64,
    _oid: &str,
    access_level: AFAccessLevel,
  ) -> Result<(), AppError> {
    // TODO: allow non workspace member to read a collab.

    // Anyone who can write to a workspace, also have full access to a collab.
    let workspace_action = match access_level {
      AFAccessLevel::ReadOnly => Action::Read,
      AFAccessLevel::ReadAndComment => Action::Read,
      AFAccessLevel::ReadAndWrite => Action::Write,
      AFAccessLevel::FullAccess => Action::Write,
    };

    let result = self
      .access_control
      .enforce(
        uid,
        ObjectType::Workspace(workspace_id.to_string()),
        workspace_action,
      )
      .await;
    match result {
      Ok(true) => Ok(()),
      Ok(false) => Err(AppError::NotEnoughPermissions),
      Err(e) => Err(e),
    }
  }

  #[instrument(level = "info", skip_all)]
  async fn update_access_level_policy(
    &self,
    _uid: &i64,
    _oid: &str,
    _level: AFAccessLevel,
  ) -> Result<(), AppError> {
    // TODO: allow non workspace member to read a collab.
    Ok(())
  }

  #[instrument(level = "info", skip_all)]
  async fn remove_access_level(&self, _uid: &i64, _oid: &str) -> Result<(), AppError> {
    // TODO: allow non workspace member to read a collab.
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
    _oid: &str,
    required_action: Action,
  ) -> Result<bool, AppError> {
    // TODO: allow non workspace member to read a collab.

    // Anyone who can write to a workspace, can also delete a collab.
    let workspace_action = match required_action {
      Action::Read => Action::Read,
      Action::Write => Action::Write,
      Action::Delete => Action::Write,
    };

    self
      .access_control
      .enforce(
        uid,
        ObjectType::Workspace(workspace_id.to_string()),
        workspace_action,
      )
      .await
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

#[cfg(test)]
mod tests {
  use database_entity::dto::AFRole;

  use crate::{
    act::Action,
    casbin::{access::AccessControl, enforcer::tests::test_enforcer},
    collab::CollabAccessControl,
    entity::{ObjectType, SubjectType},
  };

  #[tokio::test]
  pub async fn test_collab_access_control() {
    let enforcer = test_enforcer().await;
    let uid = 1;
    let workspace_id = "w1";
    let oid = "o1";
    enforcer
      .update_policy(
        SubjectType::User(uid),
        ObjectType::Workspace(workspace_id.to_string()),
        AFRole::Member,
      )
      .await
      .unwrap();
    let access_control = AccessControl::with_enforcer(enforcer);
    let collab_access_control = super::CollabAccessControlImpl::new(access_control);
    for action in vec![Action::Read, Action::Write, Action::Delete] {
      collab_access_control
        .enforce_action(workspace_id, &uid, &oid, action.clone())
        .await
        .expect(format!("Failed to enforce action: {:?}", action).as_str());
    }
  }
}
