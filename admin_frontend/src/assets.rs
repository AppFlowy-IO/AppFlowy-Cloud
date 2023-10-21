use axum::Router;
use tower_http::services::ServeDir;

pub fn router() -> Router {
  let assets_path = std::env::current_dir().unwrap();
  let assets_serve_dir = ServeDir::new(format!(
    "{}/assets",
    assets_path.to_str().expect("assets path is not valid utf8")
  ));
  Router::new().nest_service("/", assets_serve_dir)
}
