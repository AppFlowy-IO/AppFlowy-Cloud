use actix_web::web::{Data, Query};
use actix_web::{web, Scope};
use anyhow::anyhow;
use uuid::Uuid;

use app_error::AppError;
use authentication::jwt::Authorization;
use shared_entity::dto::search_dto::{SearchDocumentRequest, SearchDocumentResponseItem};
use shared_entity::response::{AppResponse, JsonAppResponse};

use crate::biz::search::search_document;
use crate::state::AppState;

pub fn search_scope() -> Scope {
  web::scope("/api/search/{workspace_id}")
    .service(web::resource("").route(web::get().to(document_search)))
}
#[tracing::instrument(skip(state, auth, payload), err)]
async fn document_search(
  auth: Authorization,
  path: web::Path<Uuid>,
  payload: Query<SearchDocumentRequest>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<Vec<SearchDocumentResponseItem>>> {
  let workspace_id = path.into_inner();
  let request = payload.into_inner();
  let user_uuid = auth.uuid()?;
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
  let openai = match &state.openai {
    Some(openai) => openai,
    None => {
      return Err(
        AppError::Internal(anyhow!(
          "Search API is not supported by this AppFlowy-Cloud instance"
        ))
        .into(),
      )
    },
  };
  let resp = search_document(&state.pg_pool, openai, uid, workspace_id, request).await?;
  Ok(AppResponse::Ok().with_data(resp).into())
}
