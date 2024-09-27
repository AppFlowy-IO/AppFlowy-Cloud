use actix_web::{
  web::{self, Data},
  Result, Scope,
};
use authentication::jwt::UserUuid;
use shared_entity::{
  dto::workspace_dto::AppAPIKey,
  response::{AppResponse, JsonAppResponse},
};
use uuid::Uuid;

use crate::{biz, state::AppState};

pub fn api_key_scope() -> Scope {
  web::scope("/api/api-key/{workspace_id}")
    .service(web::resource("").route(web::post().to(post_api_key_handler)))
}

async fn post_api_key_handler(
  workspace_id: web::Path<Uuid>,
  user_uuid: UserUuid,
  state: Data<AppState>,
  param: web::Query<AppAPIKey>,
) -> Result<JsonAppResponse<String>> {
  let req = param.into_inner();
  let generated_api_key: String =
    biz::api_key::create_api_key(&state.pg_pool, &user_uuid, &workspace_id, req.scopes).await?;
  Ok(AppResponse::Ok().with_data(generated_api_key).into())
}
