extern crate core;
use client_api::{Client, ClientConfiguration};
use dotenvy::dotenv;
use tracing::warn;
mod casbin;
mod collab;
mod gotrue;
mod user;
mod util;
mod websocket;
mod workspace;

use lazy_static::lazy_static;
use std::{borrow::Cow, env};

lazy_static! {
  pub static ref LOCALHOST_URL: Cow<'static, str> =
    get_env_var("LOCALHOST_URL", "http://localhost:8000");
  pub static ref LOCALHOST_WS: Cow<'static, str> =
    get_env_var("LOCALHOST_WS", "ws://localhost:8000/ws");
  pub static ref LOCALHOST_GOTRUE: Cow<'static, str> =
    get_env_var("LOCALHOST_GOTRUE", "http://localhost:9999");
}

fn get_env_var<'default>(key: &str, default: &'default str) -> Cow<'default, str> {
  dotenv().ok();
  match env::var(key) {
    Ok(value) => Cow::Owned(value),
    Err(_) => {
      warn!("could not read env var {}: using default: {}", key, default);
      Cow::Borrowed(default)
    },
  }
}

/// Return a client that connects to the local host. It requires to run the server locally.
/// ```shell
/// ./build/run_local_server.sh
/// ```
pub fn localhost_client() -> Client {
  Client::new(
    &LOCALHOST_URL,
    &LOCALHOST_WS,
    &LOCALHOST_GOTRUE,
    ClientConfiguration::default(),
  )
}
