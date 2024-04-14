use tokio::net::TcpListener;

use appflowy_history::application::create_app;
use appflowy_history::config::Environment;
use tracing::info;
use tracing::subscriber::set_global_default;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
  dotenvy::dotenv().ok();
  let config = appflowy_history::config::Config::from_env().expect("failed to load config");
  info!("config loaded: {:?}", &config);

  // Initialize logger
  init_subscriber(&config.app_env);

  // Start the server
  let listener = TcpListener::bind("0.0.0.0:3100")
    .await
    .expect("failed to bind to port");
  let app = create_app(config).await.unwrap();
  axum::serve(listener, app)
    .await
    .expect("failed to run server");
}

pub fn init_subscriber(app_env: &Environment) {
  let level = std::env::var("RUST_LOG").unwrap_or("info".to_string());
  let mut filters = vec![];
  filters.push(format!("collab_history={}", level));

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
}
