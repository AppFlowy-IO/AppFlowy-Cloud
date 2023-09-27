use std::pin::Pin;

use actix_http::body::BoxBody;
use actix_web::http::header::ContentType;
use actix_web::web::Payload;
use actix_web::{
  web::{self, Data},
  Scope,
};
use actix_web::{HttpResponse, Result};
use shared_entity::data::{AppResponse, JsonAppResponse};
use shared_entity::error_code::ErrorCode;
use tokio::io::AsyncRead;
use tokio_stream::StreamExt;
use tokio_util::io::StreamReader;

use crate::biz::file_storage;
use crate::{component::auth::jwt::UserUuid, state::AppState};

pub fn file_storage_scope() -> Scope {
  web::scope("/api/file_storage").service(
    web::resource("/{path}")
      .route(web::get().to(get_handler))
      .route(web::put().to(put_handler))
      .route(web::delete().to(delete_handler)),
  )
}

async fn put_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  path: web::Path<String>,
  payload: Payload,
  content_type: web::Header<ContentType>,
) -> Result<JsonAppResponse<()>> {
  let file_path = path.into_inner();
  let mime = content_type.into_inner().0;
  let mut async_read = payload_to_async_read(payload);
  file_storage::put_object(
    &state.pg_pool,
    &state.s3_bucket,
    &user_uuid,
    &file_path,
    mime,
    &mut async_read,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

async fn delete_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  path: web::Path<String>,
) -> Result<JsonAppResponse<()>> {
  let file_path = path.into_inner();
  file_storage::delete_object(&state.pg_pool, &state.s3_bucket, &user_uuid, &file_path).await?;
  Ok(AppResponse::Ok().into())
}

async fn get_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  path: web::Path<String>,
) -> Result<HttpResponse<BoxBody>> {
  let file_path = path.into_inner();
  match file_storage::get_object(&state.pg_pool, &state.s3_bucket, &user_uuid, &file_path).await {
    Ok(async_read) => Ok(HttpResponse::Ok().streaming(async_read)),
    Err(e) => match e.code {
      ErrorCode::FileNotFound => Err(actix_web::error::ErrorNotFound(e)),
      _ => Err(actix_web::error::ErrorInternalServerError(e)),
    },
  }
}

fn payload_to_async_read(payload: actix_web::web::Payload) -> Pin<Box<dyn AsyncRead>> {
  let mapped =
    payload.map(|chunk| chunk.map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e)));
  let reader = StreamReader::new(mapped);
  Box::pin(reader)
}
