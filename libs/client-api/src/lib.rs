mod http;

#[cfg(feature = "collab-sync")]
pub mod collab_sync;

pub mod notify;
pub mod ws;

pub use http::*;

pub mod error {
  pub use shared_entity::error::AppError;
  pub use shared_entity::error_code::ErrorCode;
}

pub mod entity {
  pub use gotrue_entity::*;
  pub use shared_entity::*;
}
