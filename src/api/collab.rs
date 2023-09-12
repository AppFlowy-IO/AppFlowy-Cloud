use actix_web::{web, HttpResponse, Scope};
pub fn collab_scope() -> Scope {
  web::scope("/api/collab").service(
    web::resource("/")
                .route(web::post().to(create_handler))  // Assuming you rename sign_up_handler to create_handler
                .route(web::get().to(retrieve_handler))  // Assuming you add a retrieve_handler for GET requests
                .route(web::put().to(update_handler))    // Assuming you rename sign_in_password_handler to update_handler
                .route(web::delete().to(delete_handler)), // Assuming you rename sign_out_handler to delete_handler
  )
}

async fn create_handler() -> HttpResponse {
  HttpResponse::Ok().body("create_handler")
}

async fn retrieve_handler() -> HttpResponse {
  HttpResponse::Ok().body("retrieve_handler")
}

async fn update_handler() -> HttpResponse {
  HttpResponse::Ok().body("update_handler")
}

async fn delete_handler() -> HttpResponse {
  HttpResponse::Ok().body("delete_handler")
}
