use crate::biz::workspace;
use crate::biz::workspace::permission::WorkspaceMemberAccessControl;
use crate::component::auth::jwt::UserUuid;
use crate::middleware::permission_mw::{AccessControlService, ResourcePattern};
use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::Result;
use actix_web::{web, Scope};
use database_entity::{AFWorkspaceMember, AFWorkspaces};
use shared_entity::data::{AppResponse, JsonAppResponse};
use shared_entity::dto::WorkspaceMembersParams;
use sqlx::types::uuid;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::instrument;

use uuid::Uuid;

const SCOPE_PATH: &str = "/api/workspace";
const WORKSPACE_MEMBER_PATH: &str = "{workspace_id}/member";

pub fn workspace_scope() -> Scope {
  web::scope(SCOPE_PATH)
    .service(web::resource("/list").route(web::get().to(list_handler)))
    .service(
      web::resource(WORKSPACE_MEMBER_PATH)
        .route(web::get().to(members_list_handler))
        .route(web::post().to(members_add_handler))
        .route(web::delete().to(members_remove_handler)),
    )
}

pub fn workspace_scope_access_control() -> HashMap<ResourcePattern, Arc<dyn AccessControlService>> {
  let mut access_control: HashMap<ResourcePattern, Arc<dyn AccessControlService>> = HashMap::new();
  access_control.insert(
    format!("{}/{}", SCOPE_PATH, WORKSPACE_MEMBER_PATH),
    Arc::new(WorkspaceMemberAccessControl),
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

#[instrument(skip_all, err)]
async fn members_add_handler(
  user_uuid: UserUuid,
  workspace_id: web::Path<Uuid>,
  req: Json<WorkspaceMembersParams>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  workspace::ops::add_workspace_members(
    &state.pg_pool,
    &user_uuid,
    &workspace_id,
    &req.member_emails,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(skip_all, err)]
async fn members_list_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<Vec<AFWorkspaceMember>>> {
  let ws_members =
    workspace::ops::get_workspace_members(&state.pg_pool, &user_uuid, &workspace_id).await?;
  Ok(AppResponse::Ok().with_data(ws_members).into())
}

#[instrument(skip_all, err)]
async fn members_remove_handler(
  user_uuid: UserUuid,
  req: Json<WorkspaceMembersParams>,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<()>> {
  workspace::ops::remove_workspace_members(
    &state.pg_pool,
    &user_uuid,
    &workspace_id,
    &req.member_emails,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}
