use realtime_entity::collab_msg::ClientCollabMessage;
use realtime_entity::message::RealtimeMessage;

use std::collections::BinaryHeap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::interval;
use tracing::{debug, error};
use websocket::Message;

pub type AggregateMessagesSender = tokio::sync::mpsc::Sender<Message>;
pub type AggregateMessagesReceiver = tokio::sync::mpsc::Receiver<Message>;

pub struct AggregateMessageQueue {
  maximum_payload_size: usize,
  queue: Arc<Mutex<BinaryHeap<ClientCollabMessage>>>,
}

impl AggregateMessageQueue {
  pub fn new(maximum_payload_size: usize) -> Self {
    Self {
      maximum_payload_size,
      queue: Default::default(),
    }
  }

  pub async fn push(&self, msg: ClientCollabMessage) {
    let mut lock_guard = self.queue.lock().await;
    lock_guard.push(msg);
  }

  #[allow(dead_code)]
  pub async fn clean(&self) {
    self.queue.lock().await.clear();
  }

  pub async fn set_sender(&self, sender: AggregateMessagesSender) {
    let weak_queue = Arc::downgrade(&self.queue);
    let mut interval = interval(Duration::from_secs(1));
    let maximum_payload_size = self.maximum_payload_size;
    tokio::spawn(async move {
      loop {
        interval.tick().await;
        if let Some(queue) = weak_queue.upgrade() {
          let mut lock_guard = queue.lock().await;
          let mut size = 0;
          let mut messages = Vec::new();
          while let Some(msg) = lock_guard.pop() {
            size += msg.size();
            messages.push(msg);
            if size > maximum_payload_size {
              break;
            }
          }
          drop(lock_guard);

          if messages.is_empty() {
            continue;
          }

          debug!("Aggregate messages len: {}", messages.len());
          let rt_message = RealtimeMessage::ClientCollabV1(messages);
          if let Err(e) = sender.send(Message::Binary(rt_message.into())).await {
            error!("Failed to send message: {}", e);
            break;
          }
        } else {
          break;
        }
      }
    });
  }
}
