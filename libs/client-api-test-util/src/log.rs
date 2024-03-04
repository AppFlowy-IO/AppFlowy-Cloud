#[cfg(not(target_arch = "wasm32"))]
use {
  std::sync::Once,
  tracing_subscriber::{fmt::Subscriber, util::SubscriberInitExt, EnvFilter},
};

#[cfg(not(target_arch = "wasm32"))]
pub fn setup_log() {
  static START: Once = Once::new();
  START.call_once(|| {
    let level = std::env::var("RUST_LOG").unwrap_or("trace".to_string());
    let mut filters = vec![];
    filters.push(format!("client_api={}", level));
    filters.push(format!("appflowy_cloud={}", level));
    filters.push(format!("collab={}", level));
    std::env::set_var("RUST_LOG", filters.join(","));

    let subscriber = Subscriber::builder()
      .with_ansi(true)
      .with_env_filter(EnvFilter::from_default_env())
      .finish();
    subscriber.try_init().unwrap();
  });
}

#[cfg(target_arch = "wasm32")]
pub fn setup_log() {}
