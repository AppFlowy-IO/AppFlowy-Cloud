mod api;
mod app;

use app::App;
use axum::response::Response as AxumResponse;
use axum::Router;
use axum::{
  body::{boxed, Body, BoxBody},
  extract::State,
  http::{Request, Response, StatusCode, Uri},
  response::IntoResponse,
};
// use axum_session::{SessionConfig, SessionLayer, SessionStore};
// use axum_session_auth::{AuthConfig, AuthSessionLayer, SessionSqlitePool};
use leptos::LeptosOptions;
// use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};

use leptos::{component, get_configuration, view, IntoView};
use leptos_axum::generate_route_list;
use leptos_axum::LeptosRoutes;
use tower::util::ServiceExt;
use tower_http::services::ServeDir;

#[tokio::main]
async fn main() {
  let conf = get_configuration(Some("Cargo.toml")).await.unwrap();
  let leptos_options = conf.leptos_options;
  let routes = generate_route_list(App);

  // let pool = SqlitePoolOptions::new()
  //   .connect("sqlite:Todos.db")
  //   .await
  //   .expect("Could not make pool.");

  // let session_config = SessionConfig::default().with_table_name("axum_sessions");
  // let auth_config = AuthConfig::<i64>::default();
  // let session_store =
  //   SessionStore::<SessionSqlitePool>::new(Some(pool.clone().into()), session_config);
  // session_store.initiate().await.unwrap();

  // sqlx::migrate!()
  //   .run(&pool)
  //   .await
  //   .expect("could not run SQLx migrations");

  let app = Router::new()
    .nest_service("/api", api::api_router())
    .leptos_routes(&leptos_options, routes, App)
    .fallback(file_and_error_handler)
    .with_state(leptos_options);
  axum::Server::bind(&"0.0.0.0:3000".parse().unwrap())
    .serve(app.into_make_service())
    .await
    .unwrap();
}

#[component]
pub fn NotFound() -> impl IntoView {
  view! { <h1>Not Found</h1> }
}

pub async fn file_and_error_handler(
  uri: Uri,
  State(options): State<LeptosOptions>,
  req: Request<Body>,
) -> AxumResponse {
  let root = options.site_root.clone();
  let res = get_static_file(uri.clone(), &root).await.unwrap();

  if res.status() == StatusCode::OK {
    res.into_response()
  } else {
    let handler = leptos_axum::render_app_to_stream(options.to_owned(), NotFound);
    let resp = handler(req).await.into_response();
    resp
  }
}

async fn get_static_file(uri: Uri, root: &str) -> Result<Response<BoxBody>, (StatusCode, String)> {
  let req = Request::builder()
    .uri(uri.clone())
    .body(Body::empty())
    .unwrap();
  match ServeDir::new(root).oneshot(req).await {
    Ok(res) => Ok(res.map(boxed)),
    Err(err) => Err((
      StatusCode::INTERNAL_SERVER_ERROR,
      format!("Something went wrong: {err}"),
    )),
  }
}
