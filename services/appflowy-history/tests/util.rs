use anyhow::Result;
use anyhow::{anyhow, Context};
use appflowy_history::application::run_server;
use appflowy_history::config::Config;
use assert_json_diff::{assert_json_matches_no_panic, CompareMode};
use collab::core::collab::DataSource;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_stream::client::CollabRedisStream;
use futures::future::BoxFuture;
use rand::{thread_rng, Rng};
use serde_json::{json, Value};
use sqlx::PgPool;
use std::net::SocketAddr;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::timeout;

use tonic::Status;
use tonic_proto::history::history_client::HistoryClient;
use tonic_proto::history::HistoryStatePb;

pub async fn redis_client() -> redis::Client {
  let redis_uri = "redis://localhost:6379";
  redis::Client::open(redis_uri)
    .context("failed to connect to redis")
    .unwrap()
}

pub async fn redis_stream() -> CollabRedisStream {
  let redis_client = redis_client().await;
  CollabRedisStream::new(redis_client)
    .await
    .context("failed to create stream client")
    .unwrap()
}

#[allow(dead_code)]
pub fn random_i64() -> i64 {
  let mut rng = thread_rng();
  let num: i64 = rng.gen();
  num
}

#[allow(dead_code)]
pub async fn setup_db(pool: &PgPool) -> anyhow::Result<()> {
  // Have to manually create schema and tables managed by gotrue but referenced by our
  // migration scripts.
  sqlx::query(r#"create schema auth"#).execute(pool).await?;
  sqlx::query(
    r#"
      CREATE TABLE auth.users(
        id uuid NOT NULL UNIQUE,
        deleted_at timestamptz null,
        CONSTRAINT users_pkey PRIMARY KEY (id)
      )
    "#,
  )
  .execute(pool)
  .await?;

  sqlx::migrate!("../../migrations")
    .set_ignore_missing(true)
    .run(pool)
    .await
    .unwrap();
  Ok(())
}

#[derive(Clone)]
#[allow(dead_code)]
pub struct TestRpcClient {
  pub config: Config,
  history_rpc: HistoryClient<tonic::transport::Channel>,
}

impl Deref for TestRpcClient {
  type Target = HistoryClient<tonic::transport::Channel>;

  fn deref(&self) -> &Self::Target {
    &self.history_rpc
  }
}

impl DerefMut for TestRpcClient {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.history_rpc
  }
}

impl TestRpcClient {
  pub async fn new(addr: SocketAddr, config: Config) -> Self {
    let url = format!("http://{}", addr);
    let history_rpc = HistoryClient::connect(url)
      .await
      .context("failed to connect to history server")
      .unwrap();
    Self {
      history_rpc,
      config,
    }
  }
}

#[allow(dead_code)]
pub async fn run_test_server(control_stream_key: String) -> TestRpcClient {
  let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
  let addr = listener.local_addr().unwrap();
  let mut config = Config::from_env().expect("failed to load config");
  config.stream_settings.control_key = control_stream_key;

  let cloned_config = config.clone();
  tokio::spawn(async move {
    run_server(listener, cloned_config).await.unwrap();
  });

  TestRpcClient::new(addr, config).await
}

#[allow(dead_code)]
pub async fn check_doc_state_json<'a, F>(
  object_id: &str,
  timeout_secs: u64,
  expected_json: Value,
  client_action: F,
) -> Result<()>
where
  F: Fn() -> BoxFuture<'a, Result<HistoryStatePb, Status>> + Send + Sync + 'static,
{
  let duration = Duration::from_secs(timeout_secs);
  let check_interval = Duration::from_secs(2);

  let final_json = Arc::new(Mutex::new(json!({})));
  let operation = async {
    loop {
      if let Ok(data) = client_action().await {
        let collab = Collab::new_with_source(
          CollabOrigin::Server,
          object_id,
          DataSource::DocStateV1(data.doc_state.clone()),
          vec![],
          true,
        )
        .unwrap();

        let json = collab.to_json_value();
        *final_json.lock().await = json.clone();

        if assert_json_matches_no_panic(
          &json,
          &expected_json,
          assert_json_diff::Config::new(CompareMode::Inclusive),
        )
        .is_ok()
        {
          return Ok::<(), Status>(());
        }
      }
      tokio::time::sleep(check_interval).await;
    }
  };

  if timeout(duration, operation).await.is_err() {
    eprintln!(
      "Final JSON: {}, Expected: {}",
      *final_json.lock().await,
      expected_json
    );
    Err(anyhow!("Timeout reached without matching expected JSON"))
  } else {
    Ok(())
  }
}
