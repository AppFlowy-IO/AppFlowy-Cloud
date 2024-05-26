use crate::biz::chat::ops::{create_chat, create_chat_message, delete_chat, get_chat_messages};
use crate::biz::user::auth::jwt::UserUuid;
use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::{web, HttpResponse, Scope};
use app_error::AppError;
use database_entity::dto::{
  CreateChatMessageParams, CreateChatParams, GetChatMessageParams, MessageCursor,
  RepeatedChatMessage,
};
use shared_entity::response::{AppResponse, JsonAppResponse};
use std::collections::HashMap;
use tracing::trace;
use validator::Validate;

pub fn chat_scope() -> Scope {
  web::scope("/api/chat/{workspace_id}")
    .service(web::resource("").route(web::post().to(create_chat_handler)))
    .service(
      web::resource("/{chat_id}")
        .route(web::delete().to(delete_chat_handler))
        .route(web::post().to(update_chat_handler))
        .route(web::get().to(get_chat_message_handler)),
    )
    .service(web::resource("/{chat_id}/message").route(web::post().to(post_chat_message_handler)))
}
async fn create_chat_handler(
  path: web::Path<String>,
  state: Data<AppState>,
  payload: Json<CreateChatParams>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let workspace_id = path.into_inner();
  let params = payload.into_inner();
  trace!("create new chat: {:?}", params);
  create_chat(&state.pg_pool, params, &workspace_id).await?;
  Ok(AppResponse::Ok().into())
}

async fn delete_chat_handler(
  path: web::Path<(String, String)>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let (_workspace_id, chat_id) = path.into_inner();
  delete_chat(&state.pg_pool, &chat_id).await?;
  Ok(AppResponse::Ok().into())
}

async fn update_chat_handler(
  path: web::Path<(String, String)>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let (_workspace_id, chat_id) = path.into_inner();
  delete_chat(&state.pg_pool, &chat_id).await?;
  Ok(AppResponse::Ok().into())
}

async fn post_chat_message_handler(
  state: Data<AppState>,
  path: web::Path<(String, String)>,
  payload: Json<CreateChatMessageParams>,
  uuid: UserUuid,
) -> actix_web::Result<HttpResponse> {
  let (_workspace_id, chat_id) = path.into_inner();
  let params = payload.into_inner();

  if let Err(err) = params.validate() {
    return Ok(HttpResponse::from_error(AppError::from(err)));
  }

  let uid = state.user_cache.get_user_uid(&uuid).await?;
  let message_stream = create_chat_message(
    &state.pg_pool,
    uid,
    chat_id,
    params,
    state.ai_client.clone(),
  )
  .await;
  Ok(
    HttpResponse::Ok()
      .content_type("application/json")
      .streaming(message_stream),
  )
}

async fn get_chat_message_handler(
  path: web::Path<(String, String)>,
  query: web::Query<HashMap<String, String>>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<RepeatedChatMessage>> {
  let mut params = GetChatMessageParams {
    cursor: MessageCursor::Offset(0),
    limit: query
      .get("limit")
      .and_then(|s| s.parse::<u64>().ok())
      .unwrap_or(10),
  };
  if let Some(value) = query.get("offset").and_then(|s| s.parse::<u64>().ok()) {
    params.cursor = MessageCursor::Offset(value);
  } else if let Some(value) = query.get("after").and_then(|s| s.parse::<i64>().ok()) {
    params.cursor = MessageCursor::AfterMessageId(value);
  } else if let Some(value) = query.get("before").and_then(|s| s.parse::<i64>().ok()) {
    params.cursor = MessageCursor::BeforeMessageId(value);
  } else {
    params.cursor = MessageCursor::NextBack;
  }

  trace!("get chat messages: {:?}", params);
  let (_workspace_id, chat_id) = path.into_inner();
  let messages = get_chat_messages(&state.pg_pool, params, &chat_id).await?;
  Ok(AppResponse::Ok().with_data(messages).into())
}
