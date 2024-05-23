use actix_web::web::{Data, Json};
use actix_web::{web, Scope};

use shared_entity::dto::search_dto::{SearchDocumentParams, SearchDocumentResponse};
use shared_entity::response::{AppResponse, JsonAppResponse};

use crate::biz::user::auth::jwt::Authorization;
use crate::state::AppState;

pub fn ai_tool_scope() -> Scope {
  web::scope("/api/search").service(web::resource("/").route(web::post().to(document_search)))
}
#[tracing::instrument(skip(state, auth, payload), err)]
async fn document_search(
  auth: Authorization,
  payload: Json<SearchDocumentParams>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<SearchDocumentResponse>> {
  let params = payload.into_inner();
  todo!();
  Ok(AppResponse::Ok().with_data(resp).into())
}
