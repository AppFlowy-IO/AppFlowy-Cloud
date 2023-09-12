use crate::component::auth::jwt::UserUuid;
use crate::state::State;
use actix_web::web::Data;
use actix_web::{web, HttpResponse, Scope};

pub fn collab_scope() -> Scope {
  web::scope("/api/collab").service(
    web::resource("/")
      .route(web::post().to(create_collab_handler))
      .route(web::get().to(retrieve_collab_handler))
      .route(web::put().to(update_collab_handler))
      .route(web::delete().to(delete_collab_handler)),
  )
}

async fn create_collab_handler(_uuid: UserUuid, _state: Data<State>) -> HttpResponse {
  HttpResponse::Ok().body("create_handler")
}

async fn retrieve_collab_handler() -> HttpResponse {
  HttpResponse::Ok().body("retrieve_handler")
}

async fn update_collab_handler() -> HttpResponse {
  HttpResponse::Ok().body("update_handler")
}

async fn delete_collab_handler() -> HttpResponse {
  HttpResponse::Ok().body("delete_handler")
}
