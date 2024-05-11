use crate::biz::chat::ops::{create_chat, create_chat_message, delete_chat, get_chat_messages};
use crate::biz::user::auth::jwt::UserUuid;
use crate::state::AppState;
use actix_web::web::{Data, Json};
use actix_web::{web, Scope};
use database_entity::chat::{
  CreateChatMessageParams, CreateChatParams, GetChatMessageParams, RepeatedChatMessage,
};
use shared_entity::response::{AppResponse, JsonAppResponse};

pub fn chat_scope() -> Scope {
  web::scope("/api/chat/{workspace_id}")
    .service(web::resource("/").route(web::post().to(create_chat_handler)))
    .service(
      web::resource("/{chat_id}")
        .route(web::delete().to(delete_chat_handler))
        .route(web::post().to(update_chat_handler)),
    )
    .service(
      web::resource("/{chat_id}/messages")
        .route(web::get().to(get_chat_message_handler))
        .route(web::post().to(post_chat_message_handler)),
    )
}

async fn create_chat_handler(
  path: web::Path<String>,
  state: Data<AppState>,
  payload: Json<CreateChatParams>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let workspace_id = path.into_inner();
  create_chat(&state.pg_pool, payload.into_inner(), &workspace_id).await?;
  Ok(AppResponse::Ok().into())
}

async fn delete_chat_handler(
  path: web::Path<(String, String)>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let (_, chat_id) = path.into_inner();
  delete_chat(&state.pg_pool, &chat_id).await?;
  Ok(AppResponse::Ok().into())
}

async fn update_chat_handler(
  path: web::Path<(String, String)>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<()>> {
  let (_, chat_id) = path.into_inner();
  delete_chat(&state.pg_pool, &chat_id).await?;
  Ok(AppResponse::Ok().into())
}

async fn post_chat_message_handler(
  state: Data<AppState>,
  chat_id: web::Path<String>,
  payload: Json<CreateChatMessageParams>,
  uuid: UserUuid,
) -> actix_web::Result<JsonAppResponse<()>> {
  let chat_id = chat_id.into_inner();
  let uid = state.user_cache.get_user_uid(&uuid).await?;
  create_chat_message(&state.pg_pool, uid, payload.into_inner(), &chat_id).await?;
  Ok(AppResponse::Ok().into())
}

async fn get_chat_message_handler(
  chat_id: web::Path<String>,
  state: Data<AppState>,
  payload: Json<GetChatMessageParams>,
) -> actix_web::Result<JsonAppResponse<RepeatedChatMessage>> {
  let chat_id = chat_id.into_inner();
  let messages = get_chat_messages(&state.pg_pool, payload.into_inner(), &chat_id).await?;
  Ok(AppResponse::Ok().with_data(messages).into())
}
