use crate::ws2::CollabManager;
use appflowy_proto::Rid;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::task::JoinSet;
use uuid::Uuid;

#[derive(Clone)]
pub struct SnapshotScheduler {
  schedule_queue: UnboundedSender<(Uuid, Rid)>,
}

impl SnapshotScheduler {
  /// We limit number of concurrently running snapshot tasks to avoid overwhelming the system.
  const CONCURRENCY_LIMIT: usize = 10; // only 10 workspaces at a time
  pub fn new(collab_store: Arc<CollabManager>) -> Self {
    let (schedule_queue, receiver) = tokio::sync::mpsc::unbounded_channel::<(Uuid, Rid)>();
    tokio::spawn(Self::snapshot_task(receiver, collab_store));
    Self { schedule_queue }
  }

  pub fn schedule_snapshot(&self, workspace_id: Uuid, last_message_id: Rid) {
    // free to ignore errors here, as we only drop receiver when shutting down the server
    let _ = self.schedule_queue.send((workspace_id, last_message_id));
  }

  async fn get_workspaces(
    receiver: &mut UnboundedReceiver<(Uuid, Rid)>,
  ) -> Option<HashMap<Uuid, Rid>> {
    let mut workspaces: HashMap<Uuid, Rid> = HashMap::new();
    let next = receiver.recv().await?;
    workspaces.insert(next.0, next.1);

    // Try to receive all remaining messages without blocking
    loop {
      match receiver.try_recv() {
        Ok((workspace_id, last_message_id)) => {
          let e = workspaces.entry(workspace_id).or_default();
          *e = (*e).max(last_message_id);
        },
        Err(TryRecvError::Disconnected) if workspaces.is_empty() => return None,
        Err(TryRecvError::Disconnected) => return Some(workspaces), // channel is closed, return what we have
        Err(TryRecvError::Empty) => break,                          // we emptied the queue
      }
    }
    Some(workspaces)
  }

  async fn snapshot_task(
    mut receiver: UnboundedReceiver<(Uuid, Rid)>,
    collab_store: Arc<CollabManager>,
  ) {
    while let Some(workspaces) = Self::get_workspaces(&mut receiver).await {
      tracing::trace!("snapshotting {} workspaces", workspaces.len());
      let mut tasks = JoinSet::new();
      let mut i = 0;
      for (workspace_id, last_message_id) in workspaces {
        let collab_store = collab_store.clone();
        tasks.spawn(async move {
          if let Err(err) = collab_store
            .snapshot_workspace(workspace_id, last_message_id)
            .await
          {
            tracing::error!(
              "Failed to snapshot workspace {} at message id {}: {}",
              workspace_id,
              last_message_id,
              err
            );
          }
        });
        i += 1;
        if i >= Self::CONCURRENCY_LIMIT {
          while let Some(result) = tasks.join_next().await {
            if let Err(err) = result {
              tracing::error!("Snapshot task failed: {}", err);
            }
          }
          i = 0;
        }
      }
    }
  }
}
