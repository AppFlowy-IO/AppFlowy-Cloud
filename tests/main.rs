use client_api::Client;
use std::sync::Once;
use tracing_subscriber::fmt::Subscriber;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

mod collab;
mod gotrue;
mod realtime;
mod user;

mod file_storage;
mod workspace;

pub const LOCALHOST_URL: &str = "http://localhost:8000";
pub const LOCALHOST_WS: &str = "ws://localhost:8000/ws";
pub const LOCALHOST_GOTRUE: &str = "http://localhost:9998";

/// Return a client that connects to the local host. It requires to run the server locally.
/// ```shell
/// ./build/run_local_server.sh
/// ```
pub fn localhost_client() -> Client {
  Client::new(LOCALHOST_URL, LOCALHOST_WS, LOCALHOST_GOTRUE)
}

pub const DEV_URL: &str = "https://test.appflowy.cloud";
pub const DEV_WS: &str = "wss://test.appflowy.cloud/ws";
pub const DEV_GOTRUE: &str = "https://test.appflowy.cloud/gotrue";

#[allow(dead_code)]
pub fn test_appflowy_cloud_client() -> Client {
  Client::new(DEV_URL, DEV_WS, DEV_GOTRUE)
}

pub fn setup_log() {
  static START: Once = Once::new();
  START.call_once(|| {
    let level = "trace";
    let mut filters = vec![];
    filters.push(format!("client_api={}", level));
    std::env::set_var("RUST_LOG", filters.join(","));

    let subscriber = Subscriber::builder()
      .with_ansi(true)
      .with_env_filter(EnvFilter::from_default_env())
      .finish();
    subscriber.try_init().unwrap();
  });
}
