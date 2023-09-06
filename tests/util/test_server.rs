use appflowy_cloud::application::{init_state, Application};
use appflowy_cloud::config::config::{get_configuration, DatabaseSetting};
use appflowy_cloud::state::State;
use appflowy_cloud::telemetry::{get_subscriber, init_subscriber};
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;

// use collab_plugins::disk::keys::make_collab_id_key;
// use collab_plugins::disk::rocksdb_server::RocksdbServerDiskPlugin;
// use collab_plugins::sync::server::{CollabId, COLLAB_ID_LEN};
use once_cell::sync::Lazy;
use reqwest::Certificate;
use std::path::PathBuf;

use appflowy_cloud::component::auth::RegisterResponse;
use sqlx::types::Uuid;
use sqlx::{Connection, Executor, PgConnection, PgPool};

// Ensure that the `tracing` stack is only initialised once using `once_cell`
static TRACING: Lazy<()> = Lazy::new(|| {
  let level = "trace".to_string();
  let mut filters = vec![];
  filters.push(format!("appflowy_cloud={}", level));
  filters.push(format!("collab_client_ws={}", level));
  filters.push(format!("websocket={}", level));
  filters.push(format!("collab_sync={}", level));
  // filters.push(format!("hyper={}", level));
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
  #[allow(dead_code)]
  pub cleaner: Cleaner,
}

impl TestServer {
  pub fn get_doc(&self, _object_id: &str) -> serde_json::Value {
    // let collab = MutexCollab::new(CollabOrigin::Empty, object_id, vec![]);
    // let collab_id = self.collab_id_from_object_id(object_id);
    // let plugin = RocksdbServerDiskPlugin::new(collab_id, self.state.rocksdb.clone()).unwrap();
    // collab.lock().add_plugin(Arc::new(plugin));
    // collab.initial();
    // let collab = collab.lock();
    // collab.to_json_value()
    todo!()
  }

  // pub fn collab_id_from_object_id(&self, object_id: &str) -> CollabId {
  //   let read_txn = self.state.rocksdb.read_txn();
  //   let collab_key = make_collab_id_key(object_id.as_ref());
  //   let value = read_txn.get(collab_key.as_ref()).unwrap().unwrap();
  //   let mut bytes = [0; COLLAB_ID_LEN];
  //   bytes[0..COLLAB_ID_LEN].copy_from_slice(value.as_ref());
  //   CollabId::from_be_bytes(bytes)
  // }
}

pub async fn spawn_server() -> TestServer {
  Lazy::force(&TRACING);
  let database_name = Uuid::new_v4().to_string();
  let config = {
    let mut config = get_configuration().expect("Failed to read configuration.");
    config.database.database_name = database_name.clone();
    // Use a random OS port
    config.application.port = 0;
    config.application.data_dir = PathBuf::from(format!("./data/{}", database_name));
    config
  };

  let _ = configure_database(&config.database).await;
  let state = init_state(&config).await;
  let application = Application::build(config.clone(), state.clone())
    .await
    .expect("Failed to build application");

  let port = application.port();
  let _ = tokio::spawn(async {
    let _ = application.run_until_stopped().await;
  });
  let mut builder = reqwest::Client::builder();
  let mut address = format!("http://localhost:{}", port);
  let mut ws_addr = format!("ws://localhost:{}/ws", port);
  if config.application.use_https() {
    address = format!("https://localhost:{}", port);
    ws_addr = format!("wss://localhost:{}/ws", port);
    builder = builder
      .add_root_certificate(Certificate::from_pem(include_bytes!("../../cert/cert.pem")).unwrap());
  }

  let api_client = builder
    .add_root_certificate(Certificate::from_pem(include_bytes!("../../cert/cert.pem")).unwrap())
    .redirect(reqwest::redirect::Policy::none())
    .danger_accept_invalid_certs(true)
    .cookie_store(true)
    .no_proxy()
    .build()
    .unwrap();

  let cleaner = Cleaner::new(config.application.data_dir);

  TestServer {
    state,
    api_client,
    address,
    ws_addr,
    port,
    cleaner,
  }
}

async fn configure_database(config: &DatabaseSetting) -> PgPool {
  // Create database
  let mut connection = PgConnection::connect_with(&config.without_db())
    .await
    .expect("Failed to connect to Postgres");
  connection
    .execute(&*format!(r#"CREATE DATABASE "{}";"#, config.database_name))
    .await
    .expect("Failed to create database.");

  // Migrate database
  let connection_pool = PgPool::connect_with(config.with_db())
    .await
    .expect("Failed to connect to Postgres.");

  sqlx::migrate!("./migrations")
    .run(&connection_pool)
    .await
    .expect("Failed to migrate the database");

  connection_pool
}

#[derive(serde::Serialize)]
pub struct TestUser {
  name: String,
  pub email: String,
  pub password: String,
}

impl TestUser {
  pub fn generate() -> Self {
    Self {
      name: "Me".to_string(),
      email: format!("{}@appflowy.io", Uuid::new_v4()),
      password: "Hello@AppFlowy123".to_string(),
    }
  }

  pub async fn register(&self, test_server: &TestServer) -> String {
    let url = format!("{}/api/user/register", test_server.address);
    let resp = test_server
      .api_client
      .post(&url)
      .json(self)
      .send()
      .await
      .expect("Fail to register user");

    let bytes = resp.bytes().await.unwrap();
    let response: RegisterResponse = serde_json::from_slice(&bytes).unwrap();
    response.token
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
