use actix_session::SessionExt;
use actix_session::{Session, SessionGetError};
use actix_web::dev::Payload;
use actix_web::{FromRequest, HttpRequest};

use std::future::{ready, Ready};

pub struct SessionToken(Session);

impl SessionToken {
  const TOKEN_ID_KEY: &'static str = "token";

  pub fn renew(&self) {
    self.0.renew();
  }

  pub fn get_token(&self) -> Result<Option<String>, SessionGetError> {
    self.0.get(Self::TOKEN_ID_KEY)
  }

  pub fn log_out(self) {
    self.0.purge()
  }
}

impl FromRequest for SessionToken {
  type Error = <Session as FromRequest>::Error;
  type Future = Ready<Result<SessionToken, Self::Error>>;

  fn from_request(req: &HttpRequest, _payload: &mut Payload) -> Self::Future {
    ready(Ok(SessionToken(req.get_session())))
  }
}
