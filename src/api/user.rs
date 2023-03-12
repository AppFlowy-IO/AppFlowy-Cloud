use crate::component::auth::{
    register_user, InputParamsError, LoginRequest, RegisterRequestParams,
};
use crate::domain::{UserEmail, UserName, UserPassword};
use crate::state::State;
use actix_identity::Identity;
use actix_web::web::{Data, Json, Payload};
use actix_web::Result;
use actix_web::{web, HttpResponse, Scope};

pub fn user_scope() -> Scope {
    web::scope("/api/user")
        .service(web::resource("/login").route(web::post().to(login)))
        .service(web::resource("/logout").route(web::get().to(logout)))
        .service(web::resource("/register").route(web::post().to(register)))
}

async fn login(id: Identity, req: Data<LoginRequest>, state: Data<State>) -> Result<HttpResponse> {
    todo!()
}

async fn logout(payload: Payload, id: Identity, state: Data<State>) -> Result<HttpResponse> {
    todo!()
}

#[tracing::instrument(level = "debug", skip(state))]
async fn register(req: Json<RegisterRequestParams>, state: Data<State>) -> Result<HttpResponse> {
    let params = req.into_inner();
    let name = UserName::parse(params.name)
        .map_err(|e| InputParamsError::InvalidName(e))?
        .0;
    let email = UserEmail::parse(params.email)
        .map_err(|e| InputParamsError::InvalidEmail(e))?
        .0;
    let password = UserPassword::parse(params.password)
        .map_err(|_| InputParamsError::InvalidPassword)?
        .0;

    let resp = register_user(
        state.pg_pool.clone(),
        state.cache.clone(),
        name,
        email,
        password,
    )
    .await?;

    Ok(HttpResponse::Ok().json(resp))
}
