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

    let maximum_payload_size = self.maximum_payload_size;
    let weak_queue = Arc::downgrade(&self.queue);
    let mut interval = interval(Duration::from_secs(1));

    tokio::spawn(async move {
      loop {
        tokio::select! {
          _ = rx.recv() => break,
          _ = interval.tick() => {
            if let Some(queue) = weak_queue.upgrade() {
              let messages_map = next_batch_message(maximum_payload_size, &queue).await;
              if messages_map.is_empty() {
                continue;
              }

              if cfg!(debug_assertions) {
                log_message_map(&messages_map);
              }

              send_batch_message(&sender, messages_map).await;
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
async fn send_batch_message(
  sender: &AggregateMessagesSender,
  messages_map: HashMap<String, Vec<ClientCollabMessage>>,
) {
  match RealtimeMessage::ClientCollabV2(messages_map).encode() {
    Ok(data) => {
      if let Err(e) = sender.send(Message::Binary(data)).await {
        trace!("websocket channel close:{}, stop sending messages", e);
      }
    },
    Err(err) => {
      error!("Failed to RealtimeMessage: {}", err);
    },
  }
}

#[inline]
async fn next_batch_message(
  maximum_payload_size: usize,
  queue: &Arc<Mutex<BinaryHeap<ClientCollabMessage>>>,
) -> HashMap<String, Vec<ClientCollabMessage>> {
  let mut messages_map = HashMap::new();
  let mut size = 0;
  let mut lock_guard = queue.lock().await;
  while let Some(msg) = lock_guard.pop() {
    size += msg.size();
    messages_map
      .entry(msg.object_id().to_string())
      .or_insert(vec![])
      .push(msg);
    if size > maximum_payload_size {
      break;
    }
  }

  messages_map
}

#[inline]
fn log_message_map(messages_map: &HashMap<String, Vec<ClientCollabMessage>>) {
  // Define start and end signs
  let start_sign = "----- Start of Message List -----";
  let end_sign = "------ End of Message List ------";

  let log_msg = messages_map
    .iter()
    .map(|(object_id, messages)| {
      format!(
        "object_id:{}, num of messages:{}",
        object_id,
        messages.len()
      )
    })
    .collect::<Vec<_>>()
    .join("\n"); // Joining with newline character

  // Prepend the start sign and append the end sign to the log message
  let log_msg = format!("{}\n{}\n{}", start_sign, log_msg, end_sign);

  debug!("Aggregate message list:\n{}", log_msg);
}
