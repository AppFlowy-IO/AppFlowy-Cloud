use crate::component::auth::AuthError;
use crate::state::State;

use actix_web::web::Data;
use actix_web::{web, HttpRequest, HttpResponse, Scope};

pub fn password_scope() -> Scope {
    web::scope("/api").service(web::resource("/password").route(web::post().to(change_password)))
}

async fn change_password(
    _req: HttpRequest,
    _state: Data<State>,
) -> Result<HttpResponse, AuthError> {
    todo!()
}
