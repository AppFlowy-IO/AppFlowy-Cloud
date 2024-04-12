use crate::api;
use crate::config::Config;
use crate::core::manager::OpenCollabManager;
use axum::http::Method;
use axum::Router;
use collab_stream::client::CollabRedisStream;
use redis::aio::ConnectionManager;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tracing::info;

pub async fn create_app() -> Router<()> {
  let config = Config::from_env();
  info!("config loaded: {:?}", &config);

  let redis_client = redis::Client::open(config.redis_url)
    .expect("failed to create redis client")
    .get_connection_manager()
    .await
    .expect("failed to get redis connection manager");

  let collab_redis_stream =
    CollabRedisStream::new_with_connection_manager(redis_client.clone()).await;
  let open_collab_manager = Arc::new(OpenCollabManager::new(collab_redis_stream).await);

  let state = AppState {
    redis_client,
    open_collab_manager,
  };

  let cors = CorsLayer::new()
        // allow `GET` and `POST` when accessing the resource
        .allow_methods([Method::GET, Method::POST])
        // allow requests from any origin
        .allow_origin(Any);
  Router::new()
    .nest_service("/api", api::router().with_state(state.clone()))
    .layer(ServiceBuilder::new().layer(cors))
}

#[derive(Clone)]
pub struct AppState {
  pub redis_client: ConnectionManager,
  pub open_collab_manager: Arc<OpenCollabManager>,
}
