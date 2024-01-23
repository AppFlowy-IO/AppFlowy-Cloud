mod error;
mod message;
#[cfg(not(target_arch = "wasm32"))]
mod native;
#[cfg(target_arch = "wasm32")]
mod web;

pub use error::{Error, Result};
pub use message::coding::*;
pub use message::CloseFrame;
pub use message::Message;
#[cfg(not(target_arch = "wasm32"))]
use native as ws;
#[cfg(target_arch = "wasm32")]
use web as ws;
pub use ws::WebSocketStream;

pub async fn connect_async<S: AsRef<str>>(url: S) -> Result<WebSocketStream> {
  ws::connect_async(url.as_ref()).await
}
