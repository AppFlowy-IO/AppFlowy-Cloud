use tracing::subscriber::set_global_default;
use tracing::Subscriber;
use tracing_bunyan_formatter::{BunyanFormattingLayer, JsonStorageLayer};
use tracing_log::LogTracer;

use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::{layer::SubscriberExt, EnvFilter};

pub fn get_subscriber<Sink>(
  name: String,
  env_filter: String,
  sink: Sink,
) -> impl Subscriber + Sync + Send
where
  Sink: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
  let env_filter = match EnvFilter::try_from_default_env() {
    Ok(env_filter) => {
      dbg!("Using default env filter");
      env_filter
    },
    Err(_) => EnvFilter::new(env_filter),
  };
  let formatting_layer = BunyanFormattingLayer::new(name, sink);
  tracing_subscriber::fmt()
    .with_ansi(true)
    .with_target(true)
    .with_max_level(tracing::Level::TRACE)
    .finish()
    .with(env_filter)
    .with(JsonStorageLayer)
    .with(formatting_layer)
}

/// Register a subscriber as global default to process span data.
///
/// It should only be called once!
pub fn init_subscriber(subscriber: impl Subscriber + Sync + Send) {
  LogTracer::init().expect("Failed to set logger");
  set_global_default(subscriber).expect("Failed to set subscriber");
}
