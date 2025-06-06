use client_api::{Client, ClientConfiguration};
use lazy_static::lazy_static;
use std::borrow::Cow;
use std::env;
use tracing::warn;
use uuid::Uuid;

// When running appflowy with Nginx, you need to set the environment variables in your .env file.
// LOCALHOST_URL=http://localhost
// LOCALHOST_WS=ws://localhost/ws/v1
// LOCALHOST_WS_V2=ws://localhost/ws/v2
// LOCALHOST_GOTRUE=http://localhost/gotrue

lazy_static! {
  pub static ref LOCALHOST_URL: Cow<'static, str> =
    get_env_var("LOCALHOST_URL", "http://localhost:8000");
  pub static ref LOCALHOST_WS: Cow<'static, str> =
    get_env_var("LOCALHOST_WS", "ws://localhost:8000/ws/v1");
  pub static ref LOCALHOST_WS_V2: Cow<'static, str> =
    get_env_var("LOCALHOST_WS_V2", "ws://localhost:8000/ws/v2");
  pub static ref LOCALHOST_GOTRUE: Cow<'static, str> =
    get_env_var("LOCALHOST_GOTRUE", "http://localhost:9999");
}

#[allow(dead_code)]
fn get_env_var<'default>(key: &str, default: &'default str) -> Cow<'default, str> {
  dotenvy::dotenv().ok();
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
/// ./script/run_local_server.sh
/// ```
pub fn localhost_client() -> Client {
  let device_id = Uuid::new_v4().to_string();
  localhost_client_with_device_id(&device_id)
}

pub fn localhost_client_with_device_id(device_id: &str) -> Client {
  Client::new(
    &LOCALHOST_URL,
    &LOCALHOST_WS,
    &LOCALHOST_GOTRUE,
    device_id,
    ClientConfiguration::default(),
    "0.9.0",
  )
}

pub async fn workspace_id_from_client(c: &Client) -> Uuid {
  c.get_workspaces()
    .await
    .unwrap()
    .first()
    .unwrap()
    .workspace_id
}
