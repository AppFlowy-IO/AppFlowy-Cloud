use crate::component::auth::{
    login, logout, register, ChangePasswordRequest, InputParamsError, LoggedUser, LoginRequest,
    RegisterRequest,
};
use crate::component::token_state::SessionToken;
use crate::domain::{UserEmail, UserName, UserPassword};
use crate::state::State;
use actix_identity::Identity;
use actix_session::SessionInsertError;
use actix_web::web::{Data, Json, Payload};
use actix_web::Result;
use actix_web::{web, HttpResponse, Scope};

pub fn user_scope() -> Scope {
    web::scope("/api/user")
        .service(web::resource("/login").route(web::post().to(login_handler)))
        .service(web::resource("/logout").route(web::get().to(logout_handler)))
        .service(web::resource("/register").route(web::post().to(register_handler)))
        .service(web::resource("/password").route(web::post().to(change_password)))
}

async fn login_handler(
    req: Json<LoginRequest>,
    state: Data<State>,
    session: SessionToken,
) -> Result<HttpResponse> {
    let params = req.into_inner();
    let email = UserEmail::parse(params.email)
        .map_err(|e| InputParamsError::InvalidEmail(e))?
        .0;
    let password = UserPassword::parse(params.password)
        .map_err(|_| InputParamsError::InvalidPassword)?
        .0;
    let (resp, token) = login(state.pg_pool.clone(), state.cache.clone(), email, password).await?;

    // Renews the session key, assigning existing session state to new key.
    session.renew();
    if let Err(err) = session.insert_token(token) {
        // It needs to navigate to login page in web application
        tracing::error!("Insert session failed: {}", err);
    }

    Ok(HttpResponse::Ok().json(resp))
}

async fn logout_handler(logged_user: LoggedUser, state: Data<State>) -> Result<HttpResponse> {
    logout(logged_user, state.cache.clone());
    Ok(HttpResponse::Ok().finish())
}

#[tracing::instrument(level = "debug", skip(state))]
async fn register_handler(req: Json<RegisterRequest>, state: Data<State>) -> Result<HttpResponse> {
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

    let resp = register(
        state.pg_pool.clone(),
        state.cache.clone(),
        name,
        email,
        password,
    )
    .await?;

    Ok(HttpResponse::Ok().json(resp))
}

async fn change_password(
    req: Json<ChangePasswordRequest>,
    state: Data<State>,
) -> Result<HttpResponse> {
    todo!()
}
