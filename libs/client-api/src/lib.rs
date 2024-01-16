mod http;
pub use http::*;

macro_rules! if_native {
    ($($item:item)*) => {$(
        #[cfg(not(target_arch = "wasm32"))]
        $item
    )*}
}

macro_rules! if_wasm {
    ($($item:item)*) => {$(
        #[cfg(target_arch = "wasm32")]
        $item
    )*}
}

#[cfg(feature = "collab-sync")]
pub mod collab_sync;

pub mod notify;

if_native! {
  mod native;
  #[allow(unused_imports)]
  pub use native::*;
}

if_wasm! {
  mod wasm;
  #[allow(unused_imports)]
  pub use wasm::*;
  pub use wasm::ws_wasm::*;
}

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

#[cfg(feature = "template")]
pub mod template {
  pub use workspace_template;
}
