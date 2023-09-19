use crate::biz;
use crate::state::AppState;
use shared_entity::data::{AppResponse, JsonAppResponse};
use shared_entity::dto::WorkspaceMembers;
use storage_entity::AFWorkspaces;

use crate::component::auth::jwt::UserUuid;
use actix_web::web::{Data, Json};
use actix_web::Result;
use actix_web::{web, Scope};

pub fn workspace_scope() -> Scope {
  web::scope("/api/workspace")
    .service(web::resource("/list").route(web::get().to(list_handler)))
    .service(web::resource("/member/add").route(web::post().to(members_add_handler)))
    .service(web::resource("/member/remove").route(web::post().to(members_remove_handler)))
}

async fn list_handler(
  uuid: UserUuid,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AFWorkspaces>> {
  let workspaces = biz::workspace::get_workspaces(&state.pg_pool, &uuid).await?;
  Ok(AppResponse::Ok().with_data(workspaces).into())
}

async fn members_add_handler(
  user_uuid: UserUuid,
  req: Json<WorkspaceMembers>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  biz::workspace::add_workspace_members(
    &state.pg_pool,
    &user_uuid,
    &req.workspace_uuid,
    &req.member_uids,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

async fn members_remove_handler(
  user_uuid: UserUuid,
  req: Json<WorkspaceMembers>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  biz::workspace::remove_workspace_members(
    &state.pg_pool,
    &user_uuid,
    &req.workspace_uuid,
    &req.member_uids,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}
