pub mod errors;
pub mod response;

#[cfg(feature = "with_reqwest")]
mod response_reqwest;

#[cfg(feature = "with_reqwest")]
pub use response_reqwest::*;

#[cfg(feature = "with_actix_web")]
mod response_actix_web;

#[cfg(feature = "with_actix_web")]
pub use response_actix_web::*;

pub const HEADER_TOKEN: &str = "token";
