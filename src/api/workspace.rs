use crate::biz::workspace;
use crate::biz::workspace::permission::WorkspaceOwnerAccessControl;
use crate::component::auth::jwt::UserUuid;
use crate::middleware::permission_mw::{ResourcePattern, WorkspaceAccessControlService};
use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::Result;
use actix_web::{web, Scope};
use database_entity::{AFWorkspaceMember, AFWorkspaces};
use shared_entity::data::{AppResponse, JsonAppResponse};
use shared_entity::dto::workspace_dto::*;
use sqlx::types::uuid;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::instrument;

use uuid::Uuid;

pub const WORKSPACE_ID_PATH: &str = "workspace_id";

const SCOPE_PATH: &str = "/api/workspace";
const WORKSPACE_LIST_PATH: &str = "list";
const WORKSPACE_MEMBER_PATH: &str = "{workspace_id}/member";

pub fn workspace_scope() -> Scope {
  web::scope(SCOPE_PATH)
    .service(web::resource(WORKSPACE_LIST_PATH).route(web::get().to(list_handler)))
    .service(
      web::resource(WORKSPACE_MEMBER_PATH)
        .route(web::get().to(list_workspace_members_handler))
        .route(web::post().to(add_workspace_members_handler))
        .route(web::put().to(update_workspace_member_handler))
        .route(web::delete().to(remove_workspace_member_handler)),
    )
}

pub fn workspace_scope_access_control(
) -> HashMap<ResourcePattern, Arc<dyn WorkspaceAccessControlService>> {
  let mut access_control: HashMap<ResourcePattern, Arc<dyn WorkspaceAccessControlService>> =
    HashMap::new();

  access_control.insert(
    format!("{}/{}", SCOPE_PATH, WORKSPACE_LIST_PATH),
    Arc::new(WorkspaceOwnerAccessControl),
  );

  access_control.insert(
    format!("{}/{}", SCOPE_PATH, WORKSPACE_MEMBER_PATH),
    Arc::new(WorkspaceOwnerAccessControl),
  );

  access_control
}

#[instrument(skip_all, err)]
async fn list_handler(
  uuid: UserUuid,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AFWorkspaces>> {
  let workspaces = workspace::ops::get_workspaces(&state.pg_pool, &uuid).await?;
  Ok(AppResponse::Ok().with_data(workspaces).into())
}

#[instrument(skip(payload, state), err)]
async fn add_workspace_members_handler(
  user_uuid: UserUuid,
  workspace_id: web::Path<Uuid>,
  payload: Json<CreateWorkspaceMembers>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  let create_members = payload.into_inner();
  workspace::ops::add_workspace_members(
    &state.pg_pool,
    &user_uuid,
    &workspace_id,
    create_members.0,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(skip_all, err)]
async fn list_workspace_members_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<Vec<AFWorkspaceMember>>> {
  let ws_members =
    workspace::ops::get_workspace_members(&state.pg_pool, &user_uuid, &workspace_id).await?;
  Ok(AppResponse::Ok().with_data(ws_members).into())
}

#[instrument(skip_all, err)]
async fn remove_workspace_member_handler(
  user_uuid: UserUuid,
  payload: Json<WorkspaceMembers>,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<()>> {
  let member_emails = payload
    .into_inner()
    .0
    .into_iter()
    .map(|member| member.0)
    .collect();
  workspace::ops::remove_workspace_members(
    &user_uuid,
    &state.pg_pool,
    workspace_id.into_inner(),
    member_emails,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(skip_all, err)]
async fn update_workspace_member_handler(
  payload: Json<WorkspaceMemberChangeset>,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<()>> {
  let workspace_id = workspace_id.into_inner();
  let changeset = payload.into_inner();
  workspace::ops::update_workspace_member(&state.pg_pool, &workspace_id, changeset).await?;
  Ok(AppResponse::Ok().into())
}
