use crate::errors::{ErrorCode, ServerError};
use std::{error::Error};


impl std::convert::From<reqwest::Error> for ServerError {
    fn from(error: reqwest::Error) -> Self {
        if error.is_timeout() {
            return ServerError::connect_timeout().context(error);
        }

        if error.is_request() {
            let hyper_error: Option<&hyper::Error> = error.source().unwrap().downcast_ref();
            return match hyper_error {
                None => ServerError::connect_refused().context(error),
                Some(hyper_error) => {
                    let mut code = ErrorCode::InternalError;
                    let msg = format!("{}", error);
                    if hyper_error.is_closed() {
                        code = ErrorCode::ConnectClose;
                    }

                    if hyper_error.is_connect() {
                        code = ErrorCode::ConnectRefused;
                    }

                    if hyper_error.is_canceled() {
                        code = ErrorCode::ConnectCancel;
                    }

                    if hyper_error.is_timeout() {}

                    ServerError { code, msg }
                }
            };
        }

        ServerError::internal().context(error)
    }
}