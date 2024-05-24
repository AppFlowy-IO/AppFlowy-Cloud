use actix_web::web::{Data, Json};
use actix_web::{web, Scope};
use anyhow::anyhow;
use app_error::AppError;

use crate::biz::search::search_document;
use shared_entity::dto::search_dto::{SearchDocumentRequest, SearchDocumentResponse};
use shared_entity::response::{AppResponse, JsonAppResponse};

use crate::biz::user::auth::jwt::Authorization;
use crate::state::AppState;

pub fn ai_tool_scope() -> Scope {
  web::scope("/api/search").service(web::resource("/").route(web::post().to(document_search)))
}
#[tracing::instrument(skip(state, auth, payload), err)]
async fn document_search(
  auth: Authorization,
  payload: Json<SearchDocumentRequest>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<SearchDocumentResponse>> {
  let request = payload.into_inner();
  let user_uuid = auth.uuid()?;
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  let openai = match &state.openai {
    Some(openai) => openai,
    None => return Err(AppError::Internal(anyhow!("OpenAI API key not configured")).into()),
  };
  let resp = search_document(&state.pg_pool, openai, uid, request).await?;
  Ok(AppResponse::Ok().with_data(resp).into())
}
