use std::fmt::{Display, Formatter};
use std::net::TcpListener;

use actix::{Actor, Addr};
use actix_web::dev::Server;
use actix_web::web::{Data, Path, Payload};
use actix_web::{get, web, App, HttpRequest, HttpResponse, HttpServer, Result};

use actix_web_actors::ws;
use async_trait::async_trait;
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use once_cell::sync::Lazy;
use realtime::core::{CollabManager, CollabSession};
use realtime::entities::RealtimeUser;
use serde_aux::field_attributes::deserialize_number_from_string;
use std::path::PathBuf;

use std::time::Duration;

use crate::util::log::{get_subscriber, init_subscriber};
use storage::collab::{CollabStorage, RawData};
use storage::entities::CreateCollabParams;

// Ensure that the `tracing` stack is only initialised once using `once_cell`
static TRACING: Lazy<()> = Lazy::new(|| {
  let level = "trace".to_string();
  let mut filters = vec![];
  filters.push(format!("actix_web={}", level));

  let subscriber_name = "test".to_string();
  let subscriber = get_subscriber(subscriber_name, filters.join(","), std::io::stdout);
  init_subscriber(subscriber);
});

#[derive(Clone)]
pub struct TestServer {
  pub state: State,
  pub api_client: reqwest::Client,
  pub address: String,
  pub port: u16,
  pub ws_addr: String,
  pub storage: CollabStorageMemoryImpl,
}

impl TestServer {
  pub async fn get_doc(&self, object_id: &str) -> serde_json::Value {
    let raw_data = self.storage.get_collab(object_id).await.unwrap();
    let collab =
      MutexCollab::new_with_raw_data(CollabOrigin::Empty, object_id, vec![raw_data], vec![])
        .unwrap();
    collab.async_initialize().await;

    collab.to_json_value()
  }
}

pub async fn spawn_server() -> TestServer {
  Lazy::force(&TRACING);
  let config = Config::default();
  let state = init_state(config.clone()).await;
  let storage = CollabStorageMemoryImpl {};
  let application = Application::build(config, state.clone(), storage.clone())
    .await
    .expect("Failed to build application");

  let port = application.port();
  tokio::spawn(async {
    let _ = application.run_until_stopped().await;
  });
  let builder = reqwest::Client::builder();
  let address = format!("http://localhost:{}", port);
  let ws_addr = format!("ws://localhost:{}/ws", port);

  let api_client = builder
    .redirect(reqwest::redirect::Policy::none())
    .danger_accept_invalid_certs(true)
    .cookie_store(true)
    .no_proxy()
    .build()
    .unwrap();

  TestServer {
    state,
    storage,
    api_client,
    address,
    ws_addr,
    port,
  }
}

#[derive(Clone)]
pub struct Cleaner {
  path: PathBuf,
  should_clean: bool,
}

impl Cleaner {
  fn new(path: PathBuf) -> Self {
    Self {
      path,
      should_clean: true,
    }
  }

  fn cleanup(dir: &PathBuf) {
    let _ = std::fs::remove_dir_all(dir);
  }
}

impl Drop for Cleaner {
  fn drop(&mut self) {
    if self.should_clean {
      Self::cleanup(&self.path)
    }
  }
}

pub struct Application {
  port: u16,
  server: Server,
}

impl Application {
  pub async fn build<S>(config: Config, state: State, storage: S) -> Result<Self, anyhow::Error>
  where
    S: CollabStorage + Unpin,
  {
    let address = format!("{}:{}", config.application.host, config.application.port);
    let listener = TcpListener::bind(&address)?;
    let port = listener.local_addr().unwrap().port();
    let server = run(listener, state, config, storage).await?;

    Ok(Self { port, server })
  }

  pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
    self.server.await
  }

  pub fn port(&self) -> u16 {
    self.port
  }
}

pub async fn run<S>(
  listener: TcpListener,
  state: State,
  _config: Config,
  storage: S,
) -> Result<Server, anyhow::Error>
where
  S: CollabStorage + Unpin,
{
  let collab_server = CollabManager::new(storage.clone()).unwrap().start();
  let server = HttpServer::new(move || {
    App::new()
      .service(web::scope("/ws").service(establish_ws_connection))
      .app_data(Data::new(collab_server.clone()))
      .app_data(Data::new(state.clone()))
      .app_data(Data::new(storage.clone()))
  })
  .listen(listener)?;
  Ok(server.run())
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct WebsocketSetting {
  pub heartbeat_interval: u8,
  pub client_timeout: u8,
}

impl Default for WebsocketSetting {
  fn default() -> Self {
    Self {
      heartbeat_interval: 8,
      client_timeout: 10,
    }
  }
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct Config {
  pub application: ApplicationSetting,
  pub websocket: WebsocketSetting,
}

impl Default for Config {
  fn default() -> Self {
    Self {
      application: ApplicationSetting {
        port: 8000,
        host: "0.0.0.0".to_string(),
        server_key: "".to_string(),
      },
      websocket: Default::default(),
    }
  }
}

#[derive(serde::Deserialize, Clone, Debug)]
pub struct ApplicationSetting {
  #[serde(deserialize_with = "deserialize_number_from_string")]
  pub port: u16,
  pub host: String,
  pub server_key: String,
}
#[derive(Clone)]
pub struct State {
  pub config: Config,
}
pub async fn init_state(_config: Config) -> State {
  todo!()
}

#[get("/{token}")]
pub async fn establish_ws_connection(
  request: HttpRequest,
  payload: Payload,
  token: Path<String>,
  state: Data<State>,
  server: Data<Addr<CollabManager<CollabStorageMemoryImpl>>>,
) -> Result<HttpResponse> {
  tracing::trace!("{:?}", request);
  let user = TestLoggedUser {
    user_id: token.as_str().parse().unwrap(),
  };
  let client = CollabSession::new(
    user,
    server.get_ref().clone(),
    Duration::from_secs(state.config.websocket.heartbeat_interval as u64),
    Duration::from_secs(state.config.websocket.client_timeout as u64),
  );
  match ws::start(client, &request, payload) {
    Ok(response) => Ok(response),
    Err(e) => {
      tracing::error!("🔴ws connection error: {:?}", e);
      Err(e)
    },
  }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TestLoggedUser {
  pub user_id: i64,
}

impl Display for TestLoggedUser {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_str("TestLoggedUser")
  }
}

impl RealtimeUser for TestLoggedUser {
  fn user_id(&self) -> &i64 {
    &self.user_id
  }
}

#[derive(Clone)]
pub struct CollabStorageMemoryImpl {}

#[async_trait]
impl CollabStorage for CollabStorageMemoryImpl {
  async fn is_exist(&self, _object_id: &str) -> bool {
    todo!()
  }

  async fn create_collab(&self, _params: CreateCollabParams) -> storage::collab::Result<()> {
    todo!()
  }

  async fn update_collab(&self, _object_id: &str, _data: RawData) -> storage::collab::Result<()> {
    todo!()
  }

  async fn get_collab(&self, _object_id: &str) -> storage::collab::Result<RawData> {
    todo!()
  }

  async fn delete_collab(&self, _object_id: &str) -> storage::collab::Result<()> {
    todo!()
  }
}
