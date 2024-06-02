use crate::biz::chat::ops::{
  create_chat, create_chat_message, delete_chat, generate_chat_message_answer, get_chat_messages,
  update_chat_message,
};
use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::{web, HttpResponse, Scope};
use app_error::AppError;
use appflowy_ai_client::dto::RepeatedRelatedQuestion;
use authentication::jwt::UserUuid;
use database_entity::dto::{
  ChatMessage, CreateChatMessageParams, CreateChatParams, GetChatMessageParams, MessageCursor,
  RepeatedChatMessage, UpdateChatMessageContentParams,
};
use shared_entity::response::{AppResponse, JsonAppResponse};
use std::collections::HashMap;

use tracing::{instrument, trace};
use validator::Validate;

pub fn chat_scope() -> Scope {
  web::scope("/api/chat/{workspace_id}")
    .service(web::resource("").route(web::post().to(create_chat_handler)))
    .service(
      web::resource("/{chat_id}")
        .route(web::delete().to(delete_chat_handler))
        .route(web::get().to(get_chat_message_handler)),
    )
    .service(
      web::resource("/{chat_id}/{message_id}/related_question")
        .route(web::get().to(get_related_message_handler)),
    )
    .service(
      web::resource("/{chat_id}/{message_id}/answer").route(web::get().to(generate_answer_handler)),
    )
    .service(
      web::resource("/{chat_id}/message")
        .route(web::post().to(create_chat_message_handler))
        .route(web::put().to(update_chat_message_handler)),
    )
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

async fn create_chat_message_handler(
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

async fn update_chat_message_handler(
  state: Data<AppState>,
  payload: Json<UpdateChatMessageContentParams>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let params = payload.into_inner();
  update_chat_message(&state.pg_pool, params, state.ai_client.clone()).await?;
  Ok(AppResponse::Ok().into())
}

async fn get_related_message_handler(
  path: web::Path<(String, String, i64)>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<RepeatedRelatedQuestion>> {
  let (_workspace_id, chat_id, message_id) = path.into_inner();
  let resp = state
    .ai_client
    .get_related_question(&chat_id, &message_id)
    .await
    .map_err(|err| AppError::Internal(err.into()))?;
  Ok(AppResponse::Ok().with_data(resp).into())
}

async fn generate_answer_handler(
  path: web::Path<(String, String, i64)>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<ChatMessage>> {
  let (_workspace_id, chat_id, message_id) = path.into_inner();
  let message = generate_chat_message_answer(
    &state.pg_pool,
    state.ai_client.clone(),
    message_id,
    &chat_id,
  )
  .await?;
  Ok(AppResponse::Ok().with_data(message).into())
}

#[instrument(level = "debug", skip_all, err)]
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
