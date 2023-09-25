use actix_web::http::header::ContentType;
use actix_web::Result;
use actix_web::{
  web::{self, Data},
  Scope,
};
use bytes::Bytes;
use shared_entity::data::{AppResponse, JsonAppResponse};

use crate::biz::file_storage;
use crate::{component::auth::jwt::UserUuid, state::AppState};

pub fn file_storage_scope() -> Scope {
  web::scope("/api/file_storage")
    .service(web::resource("/create/{path}").route(web::get().to(create_handler)))
}

async fn create_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  path: web::Path<String>,
  file_data: Bytes,
  content_type: web::Header<ContentType>,
) -> Result<JsonAppResponse<()>> {
  let file_path = path.into_inner();
  let bucket = &state.s3_bucket;
  let mime = content_type.into_inner().0;
  file_storage::create_object(
    &state.pg_pool,
    bucket,
    &user_uuid,
    &file_path,
    &file_data,
    mime,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}
