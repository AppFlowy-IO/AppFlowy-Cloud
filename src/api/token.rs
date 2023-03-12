use crate::component::auth::AuthError;
use crate::state::State;
use actix_identity::Identity;
use actix_web::web::{Data, Payload};
use actix_web::{web, HttpResponse, Scope};

pub fn token_scope() -> Scope {
    web::scope("api/token").service(web::resource("/renew").route(web::post().to(renew)))
}

async fn renew(
    payload: Payload,
    id: Identity,
    state: Data<State>,
) -> Result<HttpResponse, AuthError> {
    todo!()
}
