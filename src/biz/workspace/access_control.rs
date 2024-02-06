#![allow(unused)]
use crate::biz::workspace::member_listener::{WorkspaceMemberAction, WorkspaceMemberNotification};
use crate::component::auth::jwt::UserUuid;
use crate::middleware::access_control_mw::{AccessResource, HttpAccessControlService};
use actix_http::Method;
use async_trait::async_trait;
use database::user::select_uid_from_uuid;

use sqlx::{Executor, PgPool, Postgres};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use anyhow::anyhow;
use app_error::AppError;
use database_entity::dto::AFRole;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{instrument, trace, warn};
use uuid::Uuid;

#[async_trait]
pub trait WorkspaceAccessControl: Send + Sync + 'static {
  async fn get_role_from_uid(&self, uid: &i64, workspace_id: &Uuid) -> Result<AFRole, AppError>;

  async fn cache_role(&self, uid: &i64, workspace_id: &Uuid, role: AFRole) -> Result<(), AppError>;

  async fn remove_member(&self, uid: &i64, workspace_id: &Uuid) -> Result<(), AppError>;
}

/// Represents the role of the user in the workspace by the workspace id.
/// - Key: workspace id of
/// - Value: the member status of the user in the workspace
type MemberStatusByWorkspaceId = HashMap<Uuid, MemberStatus>;

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
  pub fn new(pg_pool: PgPool, listener: broadcast::Receiver<WorkspaceMemberNotification>) -> Self {
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

  pub async fn update_member(&self, uid: &i64, workspace_id: &Uuid, role: AFRole) {
    update_workspace_member_status(uid, workspace_id, role, &self.member_status_by_uid).await;
  }

  pub async fn remove_member(&self, uid: &i64, workspace_id: &Uuid) {
    if let Some(inner_map) = self.member_status_by_uid.write().await.get_mut(uid) {
      if let Entry::Occupied(mut entry) = inner_map.entry(*workspace_id) {
        entry.insert(MemberStatus::Deleted);
      }
    }
  }

  #[inline]
  async fn get_user_workspace_role(
    &self,
    uid: &i64,
    workspace_id: &Uuid,
  ) -> Result<AFRole, AppError> {
    let member_status = self
      .member_status_by_uid
      .read()
      .await
      .get(uid)
      .and_then(|map| map.get(workspace_id).cloned());

    let member_status = match member_status {
      None => {
        reload_workspace_member_status_from_db(
          uid,
          workspace_id,
          &self.pg_pool,
          &self.member_status_by_uid,
        )
        .await?
      },
      Some(status) => status,
    };

    match member_status {
      MemberStatus::Deleted => Err(AppError::NotEnoughPermissions(format!(
        "user:{} is not a member of workspace:{}",
        uid, workspace_id
      ))),
      MemberStatus::Valid(access_level) => Ok(access_level),
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
  inner_map.insert(*workspace_id, MemberStatus::Valid(role));
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
  mut listener: broadcast::Receiver<WorkspaceMemberNotification>,
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
              inner_map.insert(change.workspace_id, MemberStatus::Deleted);
            }
          },
        },
      }
    }
  });
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
    uid: &i64,
    method: Method,
  ) -> Result<(), AppError> {
    trace!("workspace_id: {:?}, uid: {:?}", workspace_id, uid);

    match self.0.get_role_from_uid(uid, workspace_id).await {
      Ok(role) => {
        if method == Method::DELETE || method == Method::POST || method == Method::PUT {
          if matches!(role, AFRole::Owner) {
            Ok(())
          } else {
            Err(AppError::NotEnoughPermissions(format!(
              "User:{:?} doesn't have the enough permission to access workspace:{}",
              uid, workspace_id
            )))
          }
        } else {
          Ok(())
        }
      },
      Err(err) => Err(AppError::NotEnoughPermissions(format!(
        "Can't find the role of the user:{:?} in the workspace:{:?}. error: {}",
        uid, workspace_id, err
      ))),
    }
  }
}
