use crate::config::config::Environment;
use actix_web::rt::task::JoinHandle;
use chrono::Local;
use tracing::subscriber::set_global_default;
use tracing_subscriber::{fmt::format::Writer, EnvFilter};

/// Register a subscriber as global default to process span data.
///
/// It should only be called once!
pub fn init_subscriber(app_env: &Environment) {
  let builder = tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::from_default_env())
    .with_target(true)
    .with_thread_ids(false)
    .with_file(true)
    .with_line_number(true);

  match app_env {
    Environment::Local => {
      #[cfg(feature = "tokio-runtime-profile")]
      console_subscriber::ConsoleLayer::builder()
        .retention(std::time::Duration::from_secs(60))
        .init();

      #[cfg(not(feature = "tokio-runtime-profile"))]
      {
        let subscriber = builder
          .with_ansi(true)
          .with_timer(CustomTime)
          .with_target(false)
          .with_file(false)
          .pretty()
          .finish();
        set_global_default(subscriber).unwrap();
      }
    },
    Environment::Production => {
      let subscriber = builder.json().finish();
      set_global_default(subscriber).unwrap();
    },
  }
}

#[allow(dead_code)]
struct CustomTime;
impl tracing_subscriber::fmt::time::FormatTime for CustomTime {
  fn format_time(&self, w: &mut Writer<'_>) -> std::fmt::Result {
    write!(w, "{}", Local::now().format("%Y-%m-%d %H:%M:%S"))
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
