use crate::api::user::UserError;
use crate::config::env::{domain, jwt_secret};
use crate::state::State;
use actix_identity::Identity;

use actix_web::web::{Data, Payload};
use actix_web::{web, FromRequest, HttpRequest, HttpResponse, Scope};
use chrono::{Duration, Local};
use futures_util::future::{ready, Ready};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

pub fn token_scope() -> Scope {
    web::scope("api/token").service(web::resource("/renew").route(web::post().to(renew)))
}

async fn renew(
    payload: Payload,
    id: Identity,
    state: Data<State>,
) -> Result<HttpResponse, UserError> {
    todo!()
}

pub const HEADER_TOKEN: &str = "token";
const DEFAULT_ALGORITHM: Algorithm = Algorithm::HS256;
pub const EXPIRED_DURATION_DAYS: i64 = 30;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claim {
    // issuer
    iss: String,
    // subject
    sub: String,
    // issue at
    iat: i64,
    // expiry
    exp: i64,
    user_id: String,
}

impl Claim {
    pub fn with_user_id(user_id: &str) -> Self {
        let domain = domain();
        Self {
            iss: domain,
            sub: "auth".to_string(),
            user_id: user_id.to_string(),
            iat: Local::now().timestamp(),
            exp: (Local::now() + Duration::days(EXPIRED_DURATION_DAYS)).timestamp(),
        }
    }

    pub fn user_id(self) -> String {
        self.user_id
    }
}

#[derive(Clone)]
pub struct Token(pub String);
impl Token {
    pub fn create_token(user_id: &str) -> Result<Self, UserError> {
        let claims = Claim::with_user_id(user_id);
        encode(
            &Header::new(DEFAULT_ALGORITHM),
            &claims,
            &EncodingKey::from_secret(jwt_secret().as_ref()),
        )
        .map(Into::into)
        .map_err(|err| UserError::InternalError)
    }

    pub fn decode_token(token: &Self) -> Result<Claim, UserError> {
        decode::<Claim>(
            &token.0,
            &DecodingKey::from_secret(jwt_secret().as_ref()),
            &Validation::new(DEFAULT_ALGORITHM),
        )
        .map(|data| Ok(data.claims))
        .map_err(|err| UserError::Unauthorized)?
    }

    pub fn parser_from_request(request: &HttpRequest) -> Result<Self, UserError> {
        match request.headers().get(HEADER_TOKEN) {
            Some(header) => match header.to_str() {
                Ok(val) => Ok(Token(val.to_owned())),
                Err(_) => Err(UserError::Unauthorized),
            },
            None => Err(UserError::Unauthorized),
        }
    }
}
impl std::convert::From<String> for Token {
    fn from(val: String) -> Self {
        Self(val)
    }
}

impl std::convert::Into<String> for Token {
    fn into(self) -> String {
        self.0
    }
}
impl FromRequest for Token {
    type Error = UserError;
    type Future = Ready<Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, payload: &mut actix_http::Payload) -> Self::Future {
        match Token::parser_from_request(req) {
            Ok(token) => ready(Ok(token)),
            Err(err) => ready(Err(err)),
        }
    }
}
