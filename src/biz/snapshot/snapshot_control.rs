use crate::state::RedisClient;
use async_stream::stream;
use database_entity::dto::InsertSnapshotParams;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::error;

pub type SnapshotCommandReceiver = tokio::sync::mpsc::Receiver<SnapshotCommand>;
pub type SnapshotCommandSender = tokio::sync::mpsc::Sender<SnapshotCommand>;
pub enum SnapshotCommand {
  InsertSnapshot(InsertSnapshotParams),
  Tick,
}

pub struct SnapshotControl {
  redis_client: Arc<Mutex<RedisClient>>,
}

impl SnapshotControl {
  pub fn new(redis_client: Arc<Mutex<RedisClient>>) -> Self {
    Self { redis_client }
  }
}

struct SnapshotCommandRunner {
  snapshot_control: Arc<SnapshotControl>,
  recv: Option<SnapshotCommandReceiver>,
}
impl SnapshotCommandRunner {
  async fn run(mut self) {
    let mut receiver = self.recv.take().expect("Only take once");
    let stream = stream! {
      while let Some(msg) = receiver.recv().await {
         yield msg;
      }
    };

    stream
      .for_each(|command| async {
        match command {
          SnapshotCommand::InsertSnapshot(params) => {
            // Insert into redis
          },
        }
      })
      .await;
  }
}
