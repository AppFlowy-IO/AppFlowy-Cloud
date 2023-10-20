use crate::biz::workspace::member_listener::{WorkspaceMemberAction, WorkspaceMemberChange};
use crate::component::auth::jwt::UserUuid;
use crate::middleware::access_control_mw::{AccessResource, HttpAccessControlService};
use actix_http::Method;
use anyhow::Context;
use async_trait::async_trait;
use database::user::select_uid_from_uuid;
use database::workspace::select_user_role;
use database_entity::AFRole;
use shared_entity::app_error::AppError;
use shared_entity::error_code::ErrorCode;
use sqlx::PgPool;
use std::collections::HashMap;
use std::ops::DerefMut;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{instrument, trace, warn};
use uuid::Uuid;

#[async_trait]
pub trait WorkspaceAccessControl: Send + Sync + 'static {
  async fn get_role_from_uuid(
    &self,
    user_uuid: &Uuid,
    workspace_id: &Uuid,
  ) -> Result<AFRole, AppError>;
  async fn get_role_from_uid(&self, uid: &i64, workspace_id: &Uuid) -> Result<AFRole, AppError>;
}

/// Represents the role of the user in the workspace by the workspace id.
/// - Key: workspace id of
/// - Value: the member status of the user in the workspace
type MemberStatusByWorkspaceId = HashMap<String, MemberStatus>;

/// Represents the role of the various workspaces for a user
/// - Key: User's UID
/// - Value: A mapping between the workspace id and the member status of the user in the workspace
type MemberStatusByUid = HashMap<i64, MemberStatusByWorkspaceId>;

#[derive(Clone, Debug)]
enum MemberStatus {
  /// Mark the user is not the member of the workspace.
  /// it don't need to query the database to get the role of the user in the workspace
  /// when the user is not the member of the workspace.
  Deleted,
  /// The user is the member of the workspace
  Valid(AFRole),
}

pub struct WorkspaceAccessControlImpl {
  pg_pool: PgPool,
  member_status_by_uid: Arc<RwLock<MemberStatusByUid>>,
}

impl WorkspaceAccessControlImpl {
  pub fn new(pg_pool: PgPool, listener: broadcast::Receiver<WorkspaceMemberChange>) -> Self {
    let member_status_by_uid = Arc::new(RwLock::new(HashMap::new()));
    spawn_listen_on_workspace_member_change(
      listener,
      pg_pool.clone(),
      member_status_by_uid.clone(),
    );

    WorkspaceAccessControlImpl {
      pg_pool,
      member_status_by_uid,
    }
  }
}

#[inline]
async fn update_workspace_member_status(
  uid: &i64,
  workspace_id: &Uuid,
  role: AFRole,
  member_status_by_uid: &Arc<RwLock<MemberStatusByUid>>,
) {
  let mut outer_map = member_status_by_uid.write().await;
  let inner_map = outer_map.entry(*uid).or_insert_with(HashMap::new);
  inner_map.insert(workspace_id.to_string(), MemberStatus::Valid(role));
}

#[inline]
async fn reload_workspace_member_status_from_db(
  uid: &i64,
  workspace_id: &Uuid,
  pg_pool: &PgPool,
  member_status_by_uid: &Arc<RwLock<MemberStatusByUid>>,
) -> Result<MemberStatus, AppError> {
  let member = database::workspace::select_workspace_member(pg_pool, uid, workspace_id).await?;
  let status = MemberStatus::Valid(member.role.clone());
  update_workspace_member_status(uid, workspace_id, member.role, member_status_by_uid).await;
  Ok(status)
}

fn spawn_listen_on_workspace_member_change(
  mut listener: broadcast::Receiver<WorkspaceMemberChange>,
  pg_pool: PgPool,
  member_status_by_uid: Arc<RwLock<MemberStatusByUid>>,
) {
  tokio::spawn(async move {
    while let Ok(change) = listener.recv().await {
      match change.action_type {
        WorkspaceMemberAction::INSERT | WorkspaceMemberAction::UPDATE => match change.new {
          None => {
            warn!("The workspace member change can't be None when the action is INSERT or UPDATE")
          },
          Some(change) => {
            if let Err(err) = reload_workspace_member_status_from_db(
              &change.uid,
              &change.workspace_id,
              &pg_pool,
              &member_status_by_uid,
            )
            .await
            {
              warn!("Failed to reload workspace member status from db: {}", err);
            }
          },
        },
        WorkspaceMemberAction::DELETE => match change.old {
          None => warn!("The workspace member change can't be None when the action is DELETE"),
          Some(change) => {
            if let Some(inner_map) = member_status_by_uid.write().await.get_mut(&change.uid) {
              inner_map.insert(change.workspace_id.to_string(), MemberStatus::Deleted);
            }
          },
        },
      }
    }
  });
}

#[async_trait]
impl WorkspaceAccessControl for WorkspaceAccessControlImpl {
  async fn get_role_from_uuid(
    &self,
    user_uuid: &Uuid,
    workspace_id: &Uuid,
  ) -> Result<AFRole, AppError> {
    let mut txn = self
      .pg_pool
      .begin()
      .await
      .context("failed to acquire a transaction to query role")?;
    let uid = select_uid_from_uuid(txn.deref_mut(), user_uuid).await?;
    let role = select_user_role(txn.deref_mut(), &uid, workspace_id).await?;
    txn
      .commit()
      .await
      .context("failed to commit transaction to query role")?;
    Ok(role)
  }

  async fn get_role_from_uid(&self, uid: &i64, workspace_id: &Uuid) -> Result<AFRole, AppError> {
    let role = select_user_role(&self.pg_pool, uid, workspace_id).await?;
    Ok(role)
  }
}

#[derive(Clone)]
pub struct WorkspaceHttpAccessControl<AC: WorkspaceAccessControl>(pub Arc<AC>);
#[async_trait]
impl<AC> HttpAccessControlService for WorkspaceHttpAccessControl<AC>
where
  AC: WorkspaceAccessControl,
{
  fn resource(&self) -> AccessResource {
    AccessResource::Workspace
  }

  #[instrument(level = "trace", skip_all, err)]
  async fn check_workspace_permission(
    &self,
    workspace_id: &Uuid,
    user_uuid: &UserUuid,
    method: Method,
  ) -> Result<(), AppError> {
    trace!(
      "workspace_id: {:?}, user_uuid: {:?}",
      workspace_id,
      user_uuid
    );

    match self.0.get_role_from_uuid(user_uuid, workspace_id).await {
      Ok(role) => {
        if method == Method::DELETE || method == Method::POST || method == Method::PUT {
          if matches!(role, AFRole::Owner) {
            Ok(())
          } else {
            Err(AppError::new(
              ErrorCode::NotEnoughPermissions,
              format!(
                "User:{:?} doesn't have the enough permission to access workspace:{}",
                user_uuid, workspace_id
              ),
            ))
          }
        } else {
          Ok(())
        }
      },
      Err(err) => Err(AppError::new(
        ErrorCode::NotEnoughPermissions,
        err.to_string(),
      )),
    }
  }
}
