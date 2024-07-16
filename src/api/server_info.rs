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
        version: env!("CARGO_PKG_VERSION").to_string(),
      })
      .into(),
  )
}
