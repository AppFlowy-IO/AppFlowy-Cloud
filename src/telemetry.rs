use actix_web::rt::task::JoinHandle;
use tracing::subscriber::set_global_default;
use tracing_subscriber::{layer::SubscriberExt, EnvFilter};

use crate::config::config::Environment;

/// Register a subscriber as global default to process span data.
///
/// It should only be called once!
pub fn init_subscriber(app_env: &Environment, filters: Vec<String>) {
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
      // let name = "appflowy_cloud".to_string();
      // let sink = std::io::stdout;
      // let formatting_layer = BunyanFormattingLayer::new(name, sink);
      let subscriber = builder.json().finish().with(env_filter);
      // .with(JsonStorageLayer)
      // .with(formatting_layer);
      set_global_default(subscriber).unwrap();
    },
  }
}

pub fn spawn_blocking_with_tracing<F, R>(f: F) -> JoinHandle<R>
where
  F: FnOnce() -> R + Send + 'static,
  R: Send + 'static,
{
  let current_span = tracing::Span::current();
  actix_web::rt::task::spawn_blocking(move || current_span.in_scope(f))
}
