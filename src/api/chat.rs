use crate::biz::user::user_verify::verify_token;
use crate::state::AppState;
use actix_web::web::Data;
use actix_web::{web, Scope};
use shared_entity::dto::auth_dto::SignInTokenResponse;
use shared_entity::response::{AppResponse, AppResponseError, JsonAppResponse};

pub fn chat_scope() -> Scope {
  web::scope("/api/chat")
    .service(web::resource("/{object_id}").route(web::post().to(post_chat_message_handler)))
    .service(
      web::resource("/{object_id}/messages").route(web::get().to(fetch_chat_messages_handler)),
    )
}

async fn post_chat_message_handler(
  object_id: web::Path<String>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<SignInTokenResponse>> {
  let object_id = object_id.into_inner();
  Ok(AppResponse::Ok().into())
}

async fn fetch_chat_messages_handler(
  object_id: web::Path<String>,
  state: Data<AppState>,
) -> actix_web::Result<JsonAppResponse<SignInTokenResponse>> {
  let object_id = object_id.into_inner();
  Ok(AppResponse::Ok().into())
}
