use crate::api::token::{Claim, Token};
use crate::state::State;
use actix_identity::Identity;
use actix_web::http::header::HeaderValue;
use actix_web::http::StatusCode;
use actix_web::web::{Data, Payload};
use actix_web::{error, web, FromRequest, HttpRequest, HttpResponse, Scope};

use derive_more::{Display, Error};
use futures_util::future::{ready, Ready};
use sqlx::types::uuid;

pub fn user_scope() -> Scope {
    web::scope("api/user")
        .service(web::resource("/login").route(web::post().to(login)))
        .service(web::resource("/logout").route(web::get().to(logout)))
        .service(web::resource("/register").route(web::post().to(register)))
        .service(web::resource("/change_password").route(web::post().to(change_password)))
}

async fn login(
    payload: Payload,
    id: Identity,
    state: Data<State>,
) -> Result<HttpResponse, UserError> {
    todo!()
}

async fn logout(
    payload: Payload,
    id: Identity,
    state: Data<State>,
) -> Result<HttpResponse, UserError> {
    todo!()
}

async fn register(
    payload: Payload,
    id: Identity,
    state: Data<State>,
) -> Result<HttpResponse, UserError> {
    todo!()
}

async fn change_password(req: HttpRequest, state: Data<State>) -> Result<HttpResponse, UserError> {
    todo!()
}

#[derive(Debug, PartialEq, Eq, Hash, Clone, Ord, PartialOrd)]
pub struct LoggedUser {
    pub user_id: String,
}

impl std::convert::From<Claim> for LoggedUser {
    fn from(c: Claim) -> Self {
        Self {
            user_id: c.user_id(),
        }
    }
}

impl LoggedUser {
    pub fn new(user_id: &str) -> Self {
        Self {
            user_id: user_id.to_owned(),
        }
    }

    pub fn from_token(token: String) -> Result<Self, UserError> {
        let user: LoggedUser = Token::decode_token(&token.into())?.into();
        Ok(user)
    }

    pub fn as_uuid(&self) -> Result<uuid::Uuid, UserError> {
        let uuid = uuid::Uuid::parse_str(&self.user_id).map_err(|e| UserError::InvalidUuid {
            uuid: self.user_id.clone(),
        })?;
        Ok(uuid)
    }
}

impl FromRequest for LoggedUser {
    type Error = UserError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, _payload: &mut actix_http::Payload) -> Self::Future {
        match Token::parser_from_request(req) {
            Ok(token) => ready(LoggedUser::from_token(token.0)),
            Err(err) => ready(Err(err)),
        }
    }
}

impl std::convert::TryFrom<&HeaderValue> for LoggedUser {
    type Error = UserError;

    fn try_from(header: &HeaderValue) -> Result<Self, Self::Error> {
        match header.to_str() {
            Ok(val) => LoggedUser::from_token(val.to_owned()),
            Err(e) => {
                tracing::error!("Header to string failed: {:?}", e);
                Err(UserError::Unauthorized)
            }
        }
    }
}

#[derive(Debug, Display, Error)]
pub enum UserError {
    #[display(fmt = "User is unauthorized")]
    Unauthorized,

    #[display(fmt = "User internal error")]
    InternalError,

    #[display(fmt = "Invalid uuid: {}", uuid)]
    InvalidUuid { uuid: String },
}

impl error::ResponseError for UserError {
    fn status_code(&self) -> StatusCode {
        match *self {
            UserError::Unauthorized => StatusCode::UNAUTHORIZED,
            UserError::InternalError => StatusCode::INTERNAL_SERVER_ERROR,
            UserError::InvalidUuid { .. } => StatusCode::UNAUTHORIZED,
        }
    }
    fn error_response(&self) -> HttpResponse {
        HttpResponse::build(self.status_code()).body(self.to_string())
    }
}
