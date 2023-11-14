mod http;

#[cfg(feature = "collab-sync")]
pub mod collab_sync;

pub mod notify;
mod retry;
pub mod ws;

pub use http::*;

pub mod error {
  pub use shared_entity::response::AppResponseError;
  pub use shared_entity::response::ErrorCode;
}

// Export all dto entities that will be used in the frontend application
pub mod entity {
  pub use database_entity::dto::*;
  pub use gotrue_entity::dto::*;
  pub use realtime_entity::user::*;
  pub use shared_entity::dto::*;
}
