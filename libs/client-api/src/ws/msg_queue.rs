use client_websocket::Message;
use collab_rt_entity::collab_msg::{ClientCollabMessage, MsgId};
use collab_rt_entity::message::RealtimeMessage;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{sleep_until, Instant};
use tracing::{debug, error, trace};

pub type AggregateMessagesSender = mpsc::Sender<Message>;
pub type AggregateMessagesReceiver = mpsc::Receiver<Message>;

pub struct AggregateMessageQueue {
  maximum_payload_size: usize,
  queue: Arc<Mutex<BinaryHeap<ClientCollabMessage>>>,
  stop_tx: Mutex<Option<mpsc::Sender<()>>>,
  seen_ids: Arc<Mutex<HashSet<String>>>,
}

impl AggregateMessageQueue {
  pub fn new(maximum_payload_size: usize) -> Self {
    Self {
      maximum_payload_size,
      queue: Default::default(),
      stop_tx: Default::default(),
      seen_ids: Arc::new(Default::default()),
    }
  }

  pub async fn push(&self, msg: Vec<ClientCollabMessage>) {
    let mut queue_guard = self.queue.lock().await;
    let mut seen_ids_guard = self.seen_ids.lock().await;

    for msg in msg.into_iter() {
      if seen_ids_guard.insert(seen_id(msg.object_id(), msg.msg_id())) {
        // Assuming ClientCollabMessage has an id() method.
        queue_guard.push(msg);
      }
    }
  }

  pub async fn clear(&self) {
    self.queue.lock().await.clear();
    self.seen_ids.lock().await.clear();
  }

  pub async fn set_sender(&self, sender: AggregateMessagesSender) {
    let (tx, mut rx) = mpsc::channel(1);
    if let Some(old_stop_tx) = self.stop_tx.lock().await.take() {
      let _ = old_stop_tx.send(()).await;
    }
    *self.stop_tx.lock().await = Some(tx);

    let maximum_payload_size = self.maximum_payload_size;
    let weak_queue = Arc::downgrade(&self.queue);
    let weak_seen_ids = Arc::downgrade(&self.seen_ids);
    let interval_duration = Duration::from_secs(2);
    let mut next_tick = Instant::now() + interval_duration;
    tokio::spawn(async move {
      loop {
        tokio::select! {
          _ = rx.recv() => break,
          _ = sleep_until(next_tick) => {
            if let Some(queue) = weak_queue.upgrade() {
              let (did_sent_seen_ids, messages_map) = next_batch_message(10, maximum_payload_size, &queue).await;
              if messages_map.is_empty() {
                continue;
              }

              if cfg!(debug_assertions) {
                log_message_map(&messages_map);
              }

              send_batch_message(&sender, messages_map).await;

              if let Some(seen_ids) = weak_seen_ids.upgrade() {
                let mut seen_lock = seen_ids.lock().await;
                for id in did_sent_seen_ids {
                  seen_lock.remove(&id);
                }
              }

              if queue.lock().await.len() > 20 {
                next_tick = Instant::now() +  Duration::from_secs(1);
              } else {
                next_tick = Instant::now() + interval_duration;
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

/// Gathers a batch of messages up to certain limits.
///
/// This function collects messages from a shared priority queue until reaching either the maximum number
/// of initial sync messages or the maximum payload size. It groups messages by their object ID.
///
/// # Arguments
/// - `maximum_init_sync`: Max number of initial sync messages allowed in a batch.
/// - `maximum_payload_size`: Max total size of messages (in bytes) allowed in a batch.
#[inline]
async fn next_batch_message(
  maximum_init_sync: usize,
  maximum_payload_size: usize,
  queue: &Arc<Mutex<BinaryHeap<ClientCollabMessage>>>,
) -> (HashSet<String>, HashMap<String, Vec<ClientCollabMessage>>) {
  let mut messages_map = HashMap::new();
  let mut size = 0;
  let mut init_sync_count = 0;
  let mut lock_guard = queue.lock().await;
  let mut seen_ids = HashSet::new();
  while let Some(msg) = lock_guard.pop() {
    size += msg.size();
    if msg.is_init_sync() {
      init_sync_count += 1;
    }
    seen_ids.insert(seen_id(msg.object_id(), msg.msg_id()));
    messages_map
      .entry(msg.object_id().to_string())
      .or_insert(vec![])
      .push(msg);

    if init_sync_count > maximum_init_sync {
      break;
    }
    if size > maximum_payload_size {
      break;
    }
  }

  (seen_ids, messages_map)
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

fn seen_id(object_id: &str, msg_id: MsgId) -> String {
  format!("{}:{}", object_id, msg_id)
}
