use crate::biz::casbin::access_control::AccessControl;
use crate::biz::casbin::access_control::{
  ActionType, ObjectType, POLICY_FIELD_INDEX_ACTION, POLICY_FIELD_INDEX_OBJECT,
  POLICY_FIELD_INDEX_USER,
};
use crate::biz::workspace::access_control::WorkspaceAccessControl;
use app_error::AppError;
use async_trait::async_trait;
use casbin::MgmtApi;
use database_entity::dto::AFRole;
use sqlx::{Executor, Postgres};
use std::ops::Deref;
use std::str::FromStr;
use tracing::instrument;
use uuid::Uuid;

#[derive(Clone)]
pub struct WorkspaceAccessControlImpl(AccessControl);

impl WorkspaceAccessControlImpl {
  pub fn new(access_control: AccessControl) -> Self {
    Self(access_control)
  }
}

impl Deref for WorkspaceAccessControlImpl {
  type Target = AccessControl;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

#[async_trait]
impl WorkspaceAccessControl for WorkspaceAccessControlImpl {
  async fn get_role_from_uid<'a, E>(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    executor: E,
  ) -> Result<AFRole, AppError>
  where
    E: Executor<'a, Database = Postgres>,
  {
    let policies = self.0.enforcer.read().await.get_filtered_policy(
      POLICY_FIELD_INDEX_OBJECT,
      vec![ObjectType::Workspace(&workspace_id.to_string()).to_string()],
    );

    let role = match policies
      .into_iter()
      .find(|p| p[POLICY_FIELD_INDEX_USER] == uid.to_string())
    {
      Some(policy) => i32::from_str(policy[POLICY_FIELD_INDEX_ACTION].as_str())
        .ok()
        .map(AFRole::from),
      None => database::workspace::select_workspace_member(executor, uid, workspace_id)
        .await
        .map(|r| r.role)
        .ok(),
    };

    role.ok_or_else(|| {
      AppError::NotEnoughPermissions(format!(
        "user:{} is not a member of workspace:{}",
        uid, workspace_id
      ))
    })
  }

  #[instrument(level = "info", skip_all)]
  async fn update_role(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
    role: AFRole,
  ) -> Result<(), AppError> {
    let _ = self
      .0
      .update(
        uid,
        &ObjectType::Workspace(&workspace_id.to_string()),
        &ActionType::Role(role),
      )
      .await?;
    Ok(())
  }

  async fn remove_role(&self, uid: &i64, workspace_id: &Uuid) -> Result<(), AppError> {
    let _ = self
      .0
      .remove(uid, &ObjectType::Workspace(&workspace_id.to_string()))
      .await?;
    Ok(())
  }
}
