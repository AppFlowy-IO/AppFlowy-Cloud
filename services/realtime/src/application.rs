use crate::api::ws_scope;
use crate::config::Config;
use crate::state::AppState;
use actix_web::dev::Server;
use actix_web::web::Data;
use actix_web::{App, HttpServer};
use anyhow::Error;
use std::net::TcpListener;
use std::sync::Arc;

pub struct Application {
  server: Server,
}

impl Application {
  pub async fn build(config: Config, state: AppState) -> Result<Self, Error> {
    let address = format!("{}:{}", config.application.host, config.application.port);
    let listener = TcpListener::bind(&address)?;
    let server = run(listener, state, config).await?;

    Ok(Self { server })
  }

  pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
    self.server.await
  }
}

pub async fn run(listener: TcpListener, state: AppState, config: Config) -> Result<Server, Error> {
  let mut server = HttpServer::new(move || {
    App::new()
      .app_data(Data::new(state.clone()))
      .service(ws_scope())
  });
  server = server.listen(listener)?;

  Ok(server.run())
}

pub async fn init_state(config: &Config) -> Result<AppState, Error> {
  let app_state = AppState {
    config: Arc::new(config.clone()),
  };
  Ok(app_state)
}
