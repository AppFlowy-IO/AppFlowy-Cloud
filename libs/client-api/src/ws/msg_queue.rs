use realtime_entity::collab_msg::ClientCollabMessage;
use realtime_entity::message::RealtimeMessage;

use std::collections::{BinaryHeap, HashMap};

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Mutex};
use tokio::time::interval;
use tracing::{debug, error, trace};
use websocket::Message;

pub type AggregateMessagesSender = mpsc::Sender<Message>;
pub type AggregateMessagesReceiver = mpsc::Receiver<Message>;

pub struct AggregateMessageQueue {
  maximum_payload_size: usize,
  queue: Arc<Mutex<BinaryHeap<ClientCollabMessage>>>,
  stop_tx: Mutex<Option<mpsc::Sender<()>>>,
}

impl AggregateMessageQueue {
  pub fn new(maximum_payload_size: usize) -> Self {
    Self {
      maximum_payload_size,
      queue: Default::default(),
      stop_tx: Default::default(),
    }
  }

  pub async fn push(&self, msg: Vec<ClientCollabMessage>) {
    let mut queue_lock_guard = self.queue.lock().await;
    for msg in msg {
      queue_lock_guard.push(msg);
    }
  }

  pub async fn clear(&self) {
    self.queue.lock().await.clear();
  }

  pub async fn set_sender(&self, sender: AggregateMessagesSender) {
    let (tx, mut rx) = mpsc::channel(1);
    if let Some(old_stop_tx) = self.stop_tx.lock().await.take() {
      let _ = old_stop_tx.send(()).await;
    }
    *self.stop_tx.lock().await = Some(tx);

    let weak_queue = Arc::downgrade(&self.queue);
    let mut interval = interval(Duration::from_secs(1));
    let maximum_payload_size = self.maximum_payload_size;

    tokio::spawn(async move {
      loop {
        tokio::select! {
          _ = rx.recv() => break,
          _ = interval.tick() => {
            if let Some(queue) = weak_queue.upgrade() {
              let mut lock_guard = queue.lock().await;
              let mut size = 0;
              let mut messages_map = HashMap::new();
              while let Some(msg) = lock_guard.pop() {
                size += msg.size();
                messages_map.entry(msg.object_id().to_string()).or_insert(vec![]).push(msg);
                if size > maximum_payload_size {
                  break;
                }
              }
              drop(lock_guard);
              if messages_map.is_empty() {
                continue;
              }

              if cfg!(debug_assertions) {
                let mut log_msg = String::new();
                for (object_id, messages) in &messages_map {
                    let part = format!("object_id:{}, num of messages:{}; ", object_id, messages.len());
                    log_msg.push_str(&part);
                }
                let log_msg = log_msg.trim_end_matches("; ").to_string();
                debug!("Aggregate messages: {}", log_msg);
              }

              let rt_message = RealtimeMessage::ClientCollabV2(messages_map);
              match rt_message.encode() {
                Ok(data) => {
                  if let Err(e) = sender.send(Message::Binary(data)).await {
                    trace!("websocket channel close:{}, stop sending messages", e);
                    break;
                  }
                }
                Err(err) => {
                  error!("Failed to RealtimeMessage: {}", err);
                }
              }
            } else {
              break;
            }
          }
        }
      }
    });
  }
}
