
use actix_web::{HttpResponse};
use actix_web::body::AnyBody;
use actix_web::{error::ResponseError};
use crate::{response::FlowyResponse, errors::ServerError};

impl std::convert::Into<HttpResponse> for FlowyResponse {
    fn into(self) -> HttpResponse {
        HttpResponse::Ok().json(self)
    }
}

impl std::convert::Into<AnyBody> for FlowyResponse {
    fn into(self) -> AnyBody {
        match serde_json::to_string(&self) {
            Ok(body) => AnyBody::from(body),
            Err(_) => AnyBody::Empty,
        }
    }
}

impl ResponseError for ServerError {
    fn error_response(&self) -> HttpResponse {
        let response: FlowyResponse = self.into();
        response.into()
    }
}