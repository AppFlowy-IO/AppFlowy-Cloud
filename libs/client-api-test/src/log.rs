#[cfg(not(target_arch = "wasm32"))]
use {
  std::sync::Once,
  tracing_subscriber::{fmt::Subscriber, util::SubscriberInitExt, EnvFilter},
};

#[cfg(not(target_arch = "wasm32"))]
fn get_bool_from_env_var(env_var_name: &str) -> bool {
  match std::env::var(env_var_name) {
    Ok(value) => match value.to_lowercase().as_str() {
      "true" | "1" => true,
      "false" | "0" => false,
      _ => false,
    },
    Err(_) => false,
  }
}

pub fn load_env() {
  // load once
  static START: Once = Once::new();
  START.call_once(|| {
    dotenvy::dotenv().ok();
  });
}

pub fn ai_test_enabled() -> bool {
  // In appflowy GitHub CI, we enable 'ai-test-enabled' feature by default, so even if the env var is not set,
  // we still enable the local ai test.
  if cfg!(feature = "ai-test-enabled") {
    return true;
  }

  load_env();
  get_bool_from_env_var("AI_TEST_ENABLED")
}

#[cfg(not(target_arch = "wasm32"))]
pub fn setup_log() {
  if get_bool_from_env_var("DISABLE_CI_TEST_LOG") {
    return;
  }

  static START: Once = Once::new();
  START.call_once(|| {
    let level = std::env::var("RUST_LOG").unwrap_or("trace".to_string());
    let mut filters = vec![];
    filters.push(format!("client_api={}", level));
    filters.push(format!("client_api_test={}", level));
    filters.push(format!("appflowy_cloud={}", level));
    filters.push(format!("sync_log={}", level));
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
