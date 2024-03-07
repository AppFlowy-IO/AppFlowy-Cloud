use realtime_entity::collab_msg::ClientCollabMessage;
use realtime_entity::message::RealtimeMessage;

use std::collections::{BinaryHeap, HashMap, HashSet};

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
  seen: Arc<Mutex<HashSet<String>>>,
  stop_tx: Mutex<Option<mpsc::Sender<()>>>,
}

impl AggregateMessageQueue {
  pub fn new(maximum_payload_size: usize) -> Self {
    Self {
      maximum_payload_size,
      queue: Default::default(),
      seen: Arc::new(Default::default()),
      stop_tx: Default::default(),
    }
  }

  pub async fn push(&self, msg: ClientCollabMessage) {
    let msg_unique_id = msg_unique_id(&msg);
    {
      let mut lock_guard = self.seen.lock().await;
      if lock_guard.contains(&msg_unique_id) {
        return;
      }
      lock_guard.insert(msg_unique_id.clone());
    }

    let mut lock_guard = self.queue.lock().await;
    lock_guard.push(msg);
  }

  pub async fn clear(&self) {
    self.queue.lock().await.clear();
    self.seen.lock().await.clear();
  }

  pub async fn set_sender(&self, sender: AggregateMessagesSender) {
    let (tx, mut rx) = mpsc::channel(1);
    if let Some(old_stop_tx) = self.stop_tx.lock().await.take() {
      let _ = old_stop_tx.send(()).await;
    }
    *self.stop_tx.lock().await = Some(tx);

    let weak_queue = Arc::downgrade(&self.queue);
    let weak_seen = Arc::downgrade(&self.seen);
    let mut interval = interval(Duration::from_secs(1));
    let maximum_payload_size = self.maximum_payload_size;

    tokio::spawn(async move {
      loop {
        tokio::select! {
          _ = rx.recv() => break,
          _ = interval.tick() => {
            if let (Some(queue), Some(seen)) = (weak_queue.upgrade(), weak_seen.upgrade()) {
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

              {
                let mut lock_guard = seen.lock().await;
                for messages in messages_map.values() {
                  for message in messages {
                    lock_guard.remove(&msg_unique_id(message));
                  }
                }
              }

              debug!("Aggregate messages len: {}", messages_map.len());
              let rt_message = RealtimeMessage::ClientCollabV1(messages_map);
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

#[inline]
fn msg_unique_id(msg: &ClientCollabMessage) -> String {
  format!("{}-{}", msg.object_id(), msg.msg_id())
}
