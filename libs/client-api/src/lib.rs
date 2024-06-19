mod http;
mod http_ai;
mod http_billing;

mod http_blob;
mod http_collab;
mod http_history;
mod http_member;
mod http_publish;
pub use http::*;

#[cfg(feature = "collab-sync")]
pub mod collab_sync;

pub mod notify;

#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(not(target_arch = "wasm32"))]
pub use native::*;

#[cfg(target_arch = "wasm32")]
mod wasm;
#[cfg(target_arch = "wasm32")]
pub use wasm::*;

#[cfg(not(target_arch = "wasm32"))]
mod http_chat;
mod http_search;
mod http_settings;
pub mod ws;

pub mod error {
  pub use shared_entity::response::AppResponseError;
  pub use shared_entity::response::ErrorCode;
}

// Export all dto entities that will be used in the frontend application
pub mod entity {
  pub use client_api_entity::*;
}

#[cfg(feature = "template")]
pub mod template {
  pub use workspace_template;
}
