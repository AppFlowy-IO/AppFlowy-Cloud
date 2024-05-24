use appflowy_ai_client::client::AppFlowyAIClient;
use std::sync::Once;
use tracing_subscriber::fmt::Subscriber;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

mod chat_test;
mod row_test;

// mod index_test;

pub fn appflowy_ai_client() -> AppFlowyAIClient {
  setup_log();
  AppFlowyAIClient::new("http://localhost:5001")
}

pub fn setup_log() {
  static START: Once = Once::new();
  START.call_once(|| {
    let level = std::env::var("RUST_LOG").unwrap_or("trace".to_string());
    let mut filters = vec![];
    filters.push(format!("appflowy_ai_client={}", level));
    std::env::set_var("RUST_LOG", filters.join(","));

    let subscriber = Subscriber::builder()
      .with_ansi(true)
      .with_env_filter(EnvFilter::from_default_env())
      .finish();
    subscriber.try_init().unwrap();
  });
}
