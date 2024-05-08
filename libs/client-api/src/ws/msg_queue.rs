use client_websocket::Message;
use collab_rt_entity::RealtimeMessage;
use collab_rt_entity::{ClientCollabMessage, MsgId};
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{sleep_until, Instant};
use tracing::{error, trace};

pub type AggregateMessagesSender = mpsc::Sender<Message>;
pub type AggregateMessagesReceiver = mpsc::Receiver<Message>;

pub struct AggregateMessageQueue {
  maximum_payload_size: usize,
  queue: Arc<Mutex<BinaryHeap<ClientCollabMessage>>>,
  stop_tx: Mutex<Option<mpsc::Sender<()>>>,
  seen_ids: Arc<Mutex<HashSet<SeenId>>>,
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
      if seen_ids_guard.insert(SeenId::from(&msg)) {
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
    let interval_duration = Duration::from_millis(1000);
    let mut next_tick = Instant::now() + interval_duration;
    tokio::spawn(async move {
      loop {
        tokio::select! {
          _ = rx.recv() => break,
          _ = sleep_until(next_tick) => {
            if let Some(queue) = weak_queue.upgrade() {
              let (num_init_sync, num_messages) = handle_tick(&sender, &queue, maximum_payload_size, weak_seen_ids.clone()).await;
              // To determine the next interval dynamically, consider factors such as the number of messages sent,
              // their total size, and the current network type. This approach allows for more nuanced interval
              // adjustments, optimizing for efficiency and responsiveness under varying conditions.
              let duration = calculate_next_tick_duration(num_messages, num_init_sync, interval_duration);
              next_tick = Instant::now() + duration;
            } else {
              break;
            }
          }
        }
      }
    });
  }
}

async fn handle_tick(
  sender: &AggregateMessagesSender,
  queue: &Arc<Mutex<BinaryHeap<ClientCollabMessage>>>,
  maximum_payload_size: usize,
  weak_seen_ids: Weak<Mutex<HashSet<SeenId>>>,
) -> (usize, usize) {
  let (did_sent_seen_ids, messages_map) = next_batch_message(10, maximum_payload_size, queue).await;
  if messages_map.is_empty() {
    return (0, 0);
  }

  log_message_map(&messages_map);

  // Send messages to server
  send_batch_message(sender, messages_map).await;

  // after sending messages, remove seen_ids
  let num_init_sync = did_sent_seen_ids
    .iter()
    .filter(|id| id.is_init_sync)
    .count();
  let num_messages = did_sent_seen_ids.len();

  if let Some(seen_ids) = weak_seen_ids.upgrade() {
    let mut seen_lock = seen_ids.lock().await;
    seen_lock.retain(|id| !did_sent_seen_ids.contains(id));
  }
  (num_init_sync, num_messages)
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
      error!("Failed to encode realtime message: {}", err);
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
) -> (HashSet<SeenId>, HashMap<String, Vec<ClientCollabMessage>>) {
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

    seen_ids.insert(SeenId::from(&msg));
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
#[cfg(feature = "sync_verbose_log")]
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
  tracing::debug!("Aggregate message list:\n{}", log_msg);
}

#[cfg(not(feature = "sync_verbose_log"))]
fn log_message_map(_messages_map: &HashMap<String, Vec<ClientCollabMessage>>) {}

#[derive(Eq, PartialEq, Hash)]
struct SeenId {
  object_id: String,
  msg_id: MsgId,
  is_init_sync: bool,
}

impl From<&ClientCollabMessage> for SeenId {
  fn from(msg: &ClientCollabMessage) -> Self {
    Self {
      object_id: msg.object_id().to_string(),
      msg_id: msg.msg_id(),
      is_init_sync: msg.is_init_sync(),
    }
  }
}

/// Calculates the duration until the next tick based on the current state.
///
/// determines the appropriate interval until the next action should be taken, considering the
/// number of messages and initial synchronizations.
///
/// - When the `test_fast_sync` feature is enabled, it always returns a fixed 1-second interval.
fn calculate_next_tick_duration(
  num_messages: usize,
  num_init_sync: usize,
  default_interval: Duration,
) -> Duration {
  if cfg!(feature = "test_util") {
    Duration::from_millis(500)
  } else if num_messages == 0 {
    Duration::from_secs(1)
  } else {
    match num_init_sync {
      0..=3 => default_interval,
      4..=7 => Duration::from_secs(4),
      _ => Duration::from_secs(6),
    }
  }
}
