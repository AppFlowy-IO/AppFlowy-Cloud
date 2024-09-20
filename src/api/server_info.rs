use actix_web::{web, Scope};
use shared_entity::dto::server_info_dto::ServerInfoResponseItem;
use shared_entity::response::{AppResponse, JsonAppResponse};

pub fn server_info_scope() -> Scope {
  web::scope("/api/server").service(web::resource("").route(web::get().to(server_info_handler)))
}

async fn server_info_handler() -> actix_web::Result<JsonAppResponse<ServerInfoResponseItem>> {
  Ok(
    AppResponse::Ok()
      .with_data(ServerInfoResponseItem {
        supported_client_features: vec![],
        minimum_supported_client_version: None,
      })
      .into(),
  )
}
