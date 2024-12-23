use crate::config::Environment;
use std::sync::Once;
use tracing::subscriber::set_global_default;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

pub fn init_subscriber(app_env: &Environment) {
  static START: Once = Once::new();
  START.call_once(|| {
    let level = std::env::var("RUST_LOG").unwrap_or("info".to_string());
    let mut filters = vec![];
    filters.push(format!("actix_web={}", level));
    filters.push(format!("collab={}", level));
    filters.push(format!("collab_sync={}", level));
    filters.push(format!("appflowy_cloud={}", level));
    filters.push(format!("collab_plugins={}", level));
    filters.push(format!("realtime={}", level));
    filters.push(format!("database={}", level));
    filters.push(format!("storage={}", level));
    filters.push(format!("gotrue={}", level));
    filters.push(format!("appflowy_collaborate={}", level));
    filters.push(format!("appflowy_ai_client={}", level));
    filters.push(format!("indexer={}", level));
    let env_filter = EnvFilter::new(filters.join(","));

    let builder = tracing_subscriber::fmt()
      .with_target(true)
      .with_max_level(tracing::Level::TRACE)
      .with_thread_ids(false)
      .with_file(false);

    match app_env {
      Environment::Local => {
        let subscriber = builder
          .with_ansi(true)
          .with_target(false)
          .with_file(false)
          .pretty()
          .finish()
          .with(env_filter);
        set_global_default(subscriber).unwrap();
      },
      Environment::Production => {
        let subscriber = builder.json().finish().with(env_filter);
        set_global_default(subscriber).unwrap();
      },
    }
  });
}
