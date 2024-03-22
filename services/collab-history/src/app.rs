use crate::config::Config;
use crate::{api, AppState};
use axum::http::Method;
use axum::Router;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

pub(crate) async fn create_app() -> Router<()> {
  let config = Config::from_env();
  info!("config loaded: {:?}", &config);

  let redis_client = redis::Client::open(config.redis_url)
    .expect("failed to create redis client")
    .get_connection_manager()
    .await
    .expect("failed to get redis connection manager");

  let state = AppState { redis_client };

  let cors = CorsLayer::new()
        // allow `GET` and `POST` when accessing the resource
        .allow_methods([Method::GET, Method::POST])
        // allow requests from any origin
        .allow_origin(Any);
  Router::new()
    .nest_service("/api", api::router().with_state(state.clone()))
    .layer(ServiceBuilder::new().layer(cors))
}
