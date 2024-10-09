use anyhow::Result;
use appflowy_worker::error::WorkerError;
use appflowy_worker::import_worker::report::{ImportNotifier, ImportProgress};
use appflowy_worker::import_worker::worker::{run_import_worker, ImportTask};
use appflowy_worker::s3_client::{S3Client, S3StreamResponse};
use aws_sdk_s3::primitives::ByteStream;
use axum::async_trait;

use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use redis::RedisResult;
use serde_json::json;
use sqlx::PgPool;
use sqlx::__rt::timeout;
use std::sync::{Arc, Once};
use std::time::Duration;
use tokio::runtime::Builder;
use tokio::task::LocalSet;

use tracing_subscriber::fmt::Subscriber;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

#[sqlx::test(migrations = false)]
async fn create_custom_task_test(pg_pool: PgPool) {
  let redis_client = redis_connection_manager().await;
  let stream_name = uuid::Uuid::new_v4().to_string();
  let notifier = Arc::new(MockNotifier::new());
  let mut task_provider = MockTaskProvider::new(redis_client.clone(), stream_name.clone());
  let _ = run_importer_worker(
    pg_pool,
    redis_client.clone(),
    notifier.clone(),
    stream_name,
    3,
  );

  let mut task_workspace_ids = vec![];
  // generate 5 tasks
  for _ in 0..5 {
    let workspace_id = uuid::Uuid::new_v4().to_string();
    task_workspace_ids.push(workspace_id.clone());
    task_provider
      .create_task(ImportTask::Custom(json!({"workspace_id": workspace_id})))
      .await;
  }

  let mut rx = notifier.subscribe();
  timeout(Duration::from_secs(30), async {
    while let Ok(task) = rx.recv().await {
      task_workspace_ids.retain(|id| {
        if let ImportProgress::Finished(result) = &task {
          if result.workspace_id == *id {
            return false;
          }
        }
        true
      });

      if task_workspace_ids.is_empty() {
        break;
      }
    }
  })
  .await
  .unwrap();
}

// #[tokio::test]
// async fn consume_group_task_test() {
//   let mut redis_client = redis_client().await;
//   let stream_name = format!("import_task_stream_{}", uuid::Uuid::new_v4());
//   let consumer_group = "import_task_group";
//   let consumer_name = "appflowy_worker";
//   let workspace_id = uuid::Uuid::new_v4().to_string();
//   let user_uuid = uuid::Uuid::new_v4().to_string();
//
//   let _: RedisResult<()> = redis_client.xgroup_create_mkstream(&stream_name, consumer_group, "0");
//   // 1. insert a task
//   let task = json!({
//       "notion": {
//          "uid": 1,
//          "user_uuid": user_uuid,
//          "workspace_id": workspace_id,
//          "s3_key": workspace_id,
//          "file_type": "zip",
//          "host": "http::localhost",
//       }
//   });
//
//   let _: () = redis_client
//     .xadd(&stream_name, "*", &[("task", task.to_string())])
//     .unwrap();
//   tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
//
//   // 2. consume a task
//   let options = StreamReadOptions::default()
//     .group(consumer_group, consumer_name)
//     .count(3);
//
//   let tasks: StreamReadReply = redis_client
//     .xread_options(&[&stream_name], &[">"], &options)
//     .unwrap();
//   assert!(!tasks.keys.is_empty());
//
//   for stream_key in tasks.keys {
//     for stream_id in stream_key.ids {
//       let task_str = match stream_id.map.get("task") {
//         Some(value) => match value {
//           Value::Data(data) => String::from_utf8_lossy(data).to_string(),
//           _ => panic!("Task field is not a string"),
//         },
//         None => continue,
//       };
//
//       let _ = from_str::<ImportTask>(&task_str).unwrap();
//       let _: () = redis_client
//         .xack(&stream_name, consumer_group, &[stream_id.id.clone()])
//         .unwrap();
//     }
//   }
// }

pub async fn redis_connection_manager() -> redis::aio::ConnectionManager {
  let redis_uri = "redis://localhost:6379";
  redis::Client::open(redis_uri)
    .expect("failed to create redis client")
    .get_connection_manager()
    .await
    .expect("failed to get redis connection manager")
}

fn run_importer_worker(
  pg_pool: PgPool,
  redis_client: ConnectionManager,
  notifier: Arc<dyn ImportNotifier>,
  stream_name: String,
  tick_interval_secs: u64,
) -> std::thread::JoinHandle<()> {
  setup_log();

  std::thread::spawn(move || {
    let runtime = Builder::new_current_thread().enable_all().build().unwrap();
    let local_set = LocalSet::new();
    let import_worker_fut = local_set.run_until(run_import_worker(
      pg_pool,
      redis_client,
      Arc::new(MockS3Client),
      notifier,
      &stream_name,
      tick_interval_secs,
    ));
    runtime.block_on(import_worker_fut).unwrap();
  })
}

struct MockTaskProvider {
  redis_client: ConnectionManager,
  stream_name: String,
}

impl MockTaskProvider {
  fn new(redis_client: ConnectionManager, stream_name: String) -> Self {
    Self {
      redis_client,
      stream_name,
    }
  }

  async fn create_task(&mut self, task: ImportTask) {
    let task = serde_json::to_string(&task).unwrap();
    let result: RedisResult<()> = self
      .redis_client
      .xadd(&self.stream_name, "*", &[("task", task.to_string())])
      .await;
    result.unwrap();
  }
}

struct MockNotifier {
  tx: tokio::sync::broadcast::Sender<ImportProgress>,
}

impl MockNotifier {
  fn new() -> Self {
    let (tx, _) = tokio::sync::broadcast::channel(100);
    Self { tx }
  }
  fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ImportProgress> {
    self.tx.subscribe()
  }
}

#[async_trait]
impl ImportNotifier for MockNotifier {
  async fn notify_progress(&self, progress: ImportProgress) {
    println!("notify_progress: {:?}", progress);
    self.tx.send(progress).unwrap();
  }
}

struct MockS3Client;

#[async_trait]
impl S3Client for MockS3Client {
  async fn get_blob_stream(&self, _object_key: &str) -> Result<S3StreamResponse, WorkerError> {
    todo!()
  }

  async fn put_blob(
    &self,
    _object_key: &str,
    _content: ByteStream,
    _content_type: Option<&str>,
  ) -> std::result::Result<(), WorkerError> {
    todo!()
  }

  async fn delete_blob(&self, _object_key: &str) -> Result<(), WorkerError> {
    Ok(())
  }
}

pub fn setup_log() {
  static START: Once = Once::new();
  START.call_once(|| {
    let level = std::env::var("RUST_LOG").unwrap_or("trace".to_string());
    let mut filters = vec![];
    filters.push(format!("appflowy_worker={}", level));
    std::env::set_var("RUST_LOG", filters.join(","));

    let subscriber = Subscriber::builder()
      .with_ansi(true)
      .with_env_filter(EnvFilter::from_default_env())
      .finish();
    subscriber.try_init().unwrap();
  });
}
