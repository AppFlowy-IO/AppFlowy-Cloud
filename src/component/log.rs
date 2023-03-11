use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

pub fn init_log() {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "debug");
    }
    let subscriber = tracing_subscriber::fmt()
        .with_target(true)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(std::io::stderr)
        .with_thread_ids(true)
        .compact()
        .finish()
        .with(EnvFilter::from_default_env());

    // let formatting_layer = BunyanFormattingLayer::new(self.name, std::io::stdout);
    // set_global_default(subscriber.with(JsonStorageLayer).with(formatting_layer)).map_err(|e| format!("{:?}", e))?;

    subscriber.try_init().unwrap()
}
