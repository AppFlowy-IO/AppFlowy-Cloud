use crate::af_spawn;
use crate::collab_sync::collab_stream::SeqNumCounter;

use crate::collab_sync::{SinkConfig, SyncError, SyncObject};
use anyhow::Error;
use collab::core::origin::{CollabClient, CollabOrigin};
use collab_rt_entity::{ClientCollabMessage, MsgId, ServerCollabMessage, SinkMessage};
use futures_util::SinkExt;
use std::collections::BinaryHeap;
use std::collections::{HashMap, HashSet};
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch, Mutex};
use tokio::time::{interval, sleep};
use tracing::{error, trace, warn};

pub(crate) const SEND_INTERVAL: Duration = Duration::from_secs(8);
pub const COLLAB_SINK_DELAY_MILLIS: u64 = 500;

pub struct CollabSink<Sink> {
  #[allow(dead_code)]
  uid: i64,
  config: SinkConfig,
  object: SyncObject,
  /// The [Sink] is used to send the messages to the remote. It might be a websocket sink or
  /// other sink that implements the [SinkExt] trait.
  sender: Arc<Mutex<Sink>>,
  /// The [SinkQueue] is used to queue the messages that are waiting to be sent to the
  /// remote. It will merge the messages if possible.
  message_queue: Arc<parking_lot::Mutex<SinkQueue<ClientCollabMessage>>>,
  sending_messages: Arc<parking_lot::Mutex<HashSet<MsgId>>>,
  /// The [watch::Sender] is used to notify the [CollabSinkRunner] to process the pending messages.
  /// Sending `false` will stop the [CollabSinkRunner].
  notifier: Arc<watch::Sender<SinkSignal>>,
  sync_state_tx: broadcast::Sender<CollabSyncState>,
  state: Arc<CollabSinkState>,
}

impl<Sink> Drop for CollabSink<Sink> {
  fn drop(&mut self) {
    if cfg!(feature = "sync_verbose_log") {
      trace!("Drop CollabSink {}", self.object.object_id);
    }

    //
    let _ = self.notifier.send(SinkSignal::Stop);
  }
}

impl<E, Sink> CollabSink<Sink>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
{
  pub fn new(
    uid: i64,
    object: SyncObject,
    sink: Sink,
    notifier: watch::Sender<SinkSignal>,
    sync_state_tx: broadcast::Sender<CollabSyncState>,
    config: SinkConfig,
  ) -> Self {
    let notifier = Arc::new(notifier);
    let sender = Arc::new(Mutex::new(sink));
    let message_queue = Arc::new(parking_lot::Mutex::new(SinkQueue::new()));
    let sending_messages = Arc::new(parking_lot::Mutex::new(HashSet::new()));
    let state = Arc::new(CollabSinkState::new());
    let mut interval = interval(SEND_INTERVAL);
    let weak_sending_messages = Arc::downgrade(&sending_messages);

    let _weak_notifier = Arc::downgrade(&notifier);
    let _origin = CollabOrigin::Client(CollabClient {
      uid,
      device_id: object.device_id.clone(),
    });

    let cloned_state = state.clone();
    let weak_notifier = Arc::downgrade(&notifier);
    af_spawn(async move {
      // Initial delay to make sure the first tick waits for SEND_INTERVAL
      sleep(SEND_INTERVAL).await;
      loop {
        interval.tick().await;
        match weak_notifier.upgrade() {
          Some(notifier) => {
            // Removing the flying messages allows for the re-sending of the top k messages in the message queue.
            if let Some(sending_messages) = weak_sending_messages.upgrade() {
              // remove all the flying messages if the last sync is expired within the SEND_INTERVAL.
              if cloned_state
                .latest_sync
                .is_time_for_next_sync(SEND_INTERVAL)
                .await
              {
                sending_messages.lock().clear();
              }
            }

            if notifier.send(SinkSignal::Proceed).is_err() {
              break;
            }
          },
          None => break,
        }
      }
    });

    Self {
      uid,
      object,
      sender,
      message_queue,
      notifier,
      sync_state_tx,
      config,
      sending_messages,
      state,
    }
  }

  /// Put the message into the queue and notify the sink to process the next message.
  /// After the [Msg] was pushed into the [SinkQueue]. The queue will pop the next msg base on
  /// its priority. And the message priority is determined by the [Msg] that implement the [Ord] and
  /// [PartialOrd] trait. Check out the [CollabMessage] for more details.
  ///
  pub fn queue_msg(&self, f: impl FnOnce(MsgId) -> ClientCollabMessage) {
    let _ = self.sync_state_tx.send(CollabSyncState::Syncing);
    let mut msg_queue = self.message_queue.lock();
    let msg_id = self.state.id_counter.next();
    let new_msg = f(msg_id);
    msg_queue.push_msg(msg_id, new_msg);
    drop(msg_queue);
    self.merge();

    // Notify the sink to process the next message after 500ms.
    let _ = self
      .notifier
      .send(SinkSignal::ProcessAfterMillis(COLLAB_SINK_DELAY_MILLIS));
  }

  /// When queue the init message, the sink will clear all the pending messages and send the init
  /// message immediately.
  pub fn queue_init_sync(&self, f: impl FnOnce(MsgId) -> ClientCollabMessage) {
    let _ = self.sync_state_tx.send(CollabSyncState::Syncing);
    self.clear();

    // When the client is connected, remove all pending messages and send the init message.
    let mut msg_queue = self.message_queue.lock();
    let msg_id = self.state.id_counter.next();
    let init_sync = f(msg_id);
    msg_queue.push_msg(msg_id, init_sync);
    self.state.did_queue_int_sync.store(true, Ordering::SeqCst);
    let _ = self.notifier.send(SinkSignal::Proceed);
  }

  pub fn did_queue_init_sync(&self) -> bool {
    self.state.did_queue_int_sync.load(Ordering::SeqCst)
  }

  /// Returns bool value to indicate whether the init sync message should be queued.
  /// The init sync message should be queued if the message queue is empty or the first message
  /// is not the init sync message.
  pub fn should_queue_init_sync(&self) -> bool {
    let msg_queue = self.message_queue.lock();
    if let Some(msg) = msg_queue.peek() {
      if msg.message().is_client_init_sync() {
        return false;
      }
    }
    true
  }

  pub fn clear(&self) {
    match self.message_queue.try_lock() {
      None => error!("failed to acquire the lock of the sink"),
      Some(mut msg_queue) => {
        msg_queue.clear();
      },
    }
    match self.sending_messages.try_lock() {
      None => error!("failed to acquire the lock of the flying message"),
      Some(mut sending_messages) => {
        sending_messages.clear();
      },
    }
  }

  pub fn pause(&self) {
    if cfg!(feature = "sync_verbose_log") {
      trace!("{}:{} pause", self.uid, self.object.object_id);
    }

    self.state.pause_ping.store(true, Ordering::SeqCst);
  }

  pub fn resume(&self) {
    if cfg!(feature = "sync_verbose_log") {
      trace!("{}:{} resume", self.uid, self.object.object_id);
    }

    self.state.pause_ping.store(false, Ordering::SeqCst);
  }

  /// Notify the sink to process the next message and mark the current message as done.
  /// Returns bool value to indicate whether the message is valid.
  pub async fn validate_response(
    &self,
    msg_id: MsgId,
    server_message: &ServerCollabMessage,
    seq_num_counter: &Arc<SeqNumCounter>,
  ) -> Result<bool, SyncError> {
    // safety: msg_id is not None
    let income_message_id = msg_id;
    let mut sending_messages = self.sending_messages.lock();

    // if the message id is not in the sending messages, it means the message is invalid.
    if !sending_messages.contains(&income_message_id) {
      return Ok(false);
    }

    let mut message_queue = self.message_queue.lock();
    let mut is_valid = false;
    // if sending_messages.contains(&income_message_id) {
    if let Some(current_item) = message_queue.pop() {
      if current_item.msg_id() != income_message_id {
        error!(
          "{} expect message id:{}, but receive:{}",
          self.object.object_id,
          current_item.msg_id(),
          income_message_id,
        );
        message_queue.push(current_item);
      } else {
        is_valid = true;
        sending_messages.remove(&income_message_id);
      }
    }

    if is_valid {
      if let ServerCollabMessage::ClientAck(ack) = server_message {
        if let Some(seq_num) = ack.get_seq_num() {
          seq_num_counter.store_ack_seq_num(seq_num);
          seq_num_counter.check_ack_broadcast_contiguous(&self.object.object_id)?;
        }
      }
    }

    // Check if all non-ping messages have been sent
    let all_non_ping_messages_sent = !message_queue
      .iter()
      .any(|item| !item.message().is_ping_sync());

    // If there are no non-ping messages left in the queue, it indicates all messages have been sent
    if all_non_ping_messages_sent {
      if let Err(err) = self.sync_state_tx.send(CollabSyncState::Finished) {
        error!(
          "Failed to send SinkState::Finished for object_id '{}': {}",
          self.object.object_id, err
        );
      }
    } else if cfg!(feature = "sync_verbose_log") {
      trace!(
        "{}: pending count:{} ids:{}",
        self.object.object_id,
        message_queue.len(),
        message_queue
          .iter()
          .map(|item| item.msg_id().to_string())
          .collect::<Vec<_>>()
          .join(",")
      );
    }

    Ok(is_valid)
  }

  async fn process_next_msg(&self) {
    let items = {
      let (mut msg_queue, mut sending_messages) = match (
        self.message_queue.try_lock(),
        self.sending_messages.try_lock(),
      ) {
        (Some(msg_queue), Some(sending_messages)) => (msg_queue, sending_messages),
        _ => {
          // If acquire the lock failed, try later
          if cfg!(feature = "sync_verbose_log") {
            trace!(
              "{}: failed to acquire the lock of the sink, retry later",
              self.object.object_id
            );
          }
          retry_later(Arc::downgrade(&self.notifier));
          return;
        },
      };
      get_next_batch_item(&self.state, &mut sending_messages, &mut msg_queue)
    };
    self.send_immediately(items).await;
  }

  async fn send_immediately(&self, items: Vec<QueueItem<ClientCollabMessage>>) {
    if items.is_empty() {
      return;
    }

    let message_ids = items.iter().map(|item| item.msg_id()).collect::<Vec<_>>();
    let messages = items
      .into_iter()
      .map(|item| item.into_message())
      .collect::<Vec<_>>();
    match self.sender.try_lock() {
      Ok(mut sender) => {
        self.state.latest_sync.update_timestamp().await;
        match sender.send(messages).await {
          Ok(_) => {
            if cfg!(feature = "sync_verbose_log") {
              trace!(
                "🔥client sending {} messages {:?}",
                self.object.object_id,
                message_ids
              );
            }
          },
          Err(err) => {
            error!("Failed to send error: {:?}", err.into());
            self
              .sending_messages
              .lock()
              .retain(|id| !message_ids.contains(id));
          },
        }
      },
      Err(_) => {
        warn!("failed to acquire the lock of the sink, retry later");
        self
          .sending_messages
          .lock()
          .retain(|id| !message_ids.contains(id));
        retry_later(Arc::downgrade(&self.notifier));
      },
    }
  }

  fn merge(&self) {
    if let (Some(sending_messages), Some(mut msg_queue)) = (
      self.sending_messages.try_lock(),
      self.message_queue.try_lock(),
    ) {
      let mut items: Vec<QueueItem<ClientCollabMessage>> = Vec::with_capacity(msg_queue.len());
      let mut merged_ids = HashMap::new();
      while let Some(next) = msg_queue.pop() {
        // If the message is in the flying messages, it means the message is sending to the remote.
        // So don't merge the message.
        if sending_messages.contains(&next.msg_id()) {
          items.push(next);
          continue;
        }

        // Try to merge the next message with the last message. Only merge when:
        // 1. The last message is not in the flying messages.
        // 2. The last message can be merged.
        // 3. The last message's payload size is less than the maximum payload size.
        if let Some(last) = items.last_mut() {
          if !sending_messages.contains(&last.msg_id())
            && last.message().payload_size() < self.config.maximum_payload_size
            && last.mergeable()
            && last.merge(&next, &self.config.maximum_payload_size).is_ok()
          {
            merged_ids
              .entry(last.msg_id())
              .or_insert(vec![])
              .push(next.msg_id());

            // If the last message is merged with the next message, don't push the next message
            continue;
          }
        }
        items.push(next);
      }

      if cfg!(feature = "sync_verbose_log") {
        for (msg_id, merged_ids) in merged_ids {
          trace!(
            "{}: merged {:?} messages into: {:?}",
            self.object.object_id,
            merged_ids,
            msg_id
          );
        }
      }
      msg_queue.extend(items);
    }
  }

  /// Notify the sink to process the next message.
  pub(crate) fn notify_next(&self) {
    let _ = self.notifier.send(SinkSignal::Proceed);
  }
}

fn get_next_batch_item(
  state: &Arc<CollabSinkState>,
  sending_messages: &mut HashSet<MsgId>,
  msg_queue: &mut SinkQueue<ClientCollabMessage>,
) -> Vec<QueueItem<ClientCollabMessage>> {
  let mut next_sending_items = vec![];
  let mut requeue_items = vec![];

  while let Some(item) = msg_queue.pop() {
    // If we've already selected 20 items for sending, or if the current message
    // is already being sent (exists in sending_messages), we requeue the current item
    // and break out of the loop to prevent sending too many messages at once or
    // sending the same message twice.
    if next_sending_items.len() >= 20 || sending_messages.contains(&item.msg_id()) {
      requeue_items.push(item);
      break;
    }

    // Check if the current item is an initial synchronization message.
    let is_init_sync = item.message().is_client_init_sync();

    // Determine if the message should be sent. Messages are sent if the initial sync
    // has already been queued (did_queue_int_sync is true) or if it's an initial sync message.
    // This ensures that initial sync messages are prioritized and that other messages
    // are only sent after the initial sync has been queued.
    if state.did_queue_int_sync.load(Ordering::SeqCst) || is_init_sync {
      next_sending_items.push(item.clone());
      requeue_items.push(item);
      // If the current item is an initial sync message, we've prioritized it for sending,
      // so break the loop to handle its sending immediately.
      if is_init_sync {
        break;
      }
    } else {
      // If the current message does not meet the conditions for immediate sending
      // (e.g., initial sync hasn't been queued yet and this isn't an initial sync message),
      // we requeue it to attempt sending later.
      requeue_items.push(item);
    }
  }

  msg_queue.extend(requeue_items);
  let message_ids = next_sending_items
    .iter()
    .map(|item| item.msg_id())
    .collect::<Vec<_>>();
  sending_messages.extend(message_ids);
  next_sending_items
}

fn retry_later(weak_notifier: Weak<watch::Sender<SinkSignal>>) {
  if let Some(notifier) = weak_notifier.upgrade() {
    let _ = notifier.send(SinkSignal::ProcessAfterMillis(200));
  }
}

pub struct CollabSinkRunner;

impl CollabSinkRunner {
  /// The runner will stop if the [CollabSink] was dropped or the notifier was closed.
  pub async fn run<E, Sink>(
    weak_sink: Weak<CollabSink<Sink>>,
    mut notifier: watch::Receiver<SinkSignal>,
  ) where
    E: Into<anyhow::Error> + Send + Sync + 'static,
    Sink: SinkExt<Vec<ClientCollabMessage>, Error = E> + Send + Sync + Unpin + 'static,
  {
    loop {
      // stops the runner if the notifier was closed.
      if notifier.changed().await.is_err() {
        break;
      }
      if let Some(sync_sink) = weak_sink.upgrade() {
        let value = notifier.borrow().clone();
        match value {
          SinkSignal::Stop => break,
          SinkSignal::Proceed => {
            sync_sink.process_next_msg().await;
          },
          SinkSignal::ProcessAfterMillis(millis) => {
            sleep(Duration::from_millis(millis)).await;
            sync_sink.process_next_msg().await;
          },
        }
      } else {
        break;
      }
    }
  }
}

pub trait MsgIdCounter: Send + Sync + 'static {
  /// Get the next message id. The message id should be unique.
  fn next(&self) -> MsgId;
}

#[derive(Debug, Default)]
pub struct DefaultMsgIdCounter(Arc<AtomicU64>);

impl DefaultMsgIdCounter {
  pub fn new() -> Self {
    Self::default()
  }
  pub(crate) fn next(&self) -> MsgId {
    self.0.fetch_add(1, Ordering::SeqCst)
  }
}

pub(crate) struct SyncTimestamp {
  last_sync: Mutex<Instant>,
}

impl SyncTimestamp {
  fn new() -> Self {
    let now = Instant::now();
    SyncTimestamp {
      last_sync: Mutex::new(now.checked_sub(Duration::from_secs(60)).unwrap_or(now)),
    }
  }

  /// Indicate the duration is passed since the last sync. The last sync timestamp will be updated
  /// after sending a new message
  pub async fn is_time_for_next_sync(&self, duration: Duration) -> bool {
    Instant::now().duration_since(*self.last_sync.lock().await) > duration
  }

  async fn update_timestamp(&self) {
    let mut last_sync_locked = self.last_sync.lock().await;
    *last_sync_locked = Instant::now();
  }
}

pub(crate) struct CollabSinkState {
  pub(crate) latest_sync: SyncTimestamp,
  pub(crate) pause_ping: AtomicBool,
  pub(crate) id_counter: DefaultMsgIdCounter,
  pub(crate) did_queue_int_sync: AtomicBool,
}

impl CollabSinkState {
  fn new() -> Self {
    let msg_id_counter = DefaultMsgIdCounter::new();
    CollabSinkState {
      latest_sync: SyncTimestamp::new(),
      pause_ping: AtomicBool::new(false),
      id_counter: msg_id_counter,
      did_queue_int_sync: Default::default(),
    }
  }
}

#[derive(Clone, Debug)]
pub enum CollabSyncState {
  /// The sink is syncing the messages to the remote.
  Syncing,
  /// All the messages are synced to the remote.
  Finished,
}

impl CollabSyncState {
  pub fn is_syncing(&self) -> bool {
    matches!(self, CollabSyncState::Syncing)
  }
}

#[derive(Clone)]
pub enum SinkSignal {
  Stop,
  Proceed,
  ProcessAfterMillis(u64),
}

pub(crate) struct SinkQueue<Msg> {
  queue: BinaryHeap<QueueItem<Msg>>,
}

impl<Msg> SinkQueue<Msg>
where
  Msg: SinkMessage,
{
  pub(crate) fn new() -> Self {
    Self {
      queue: Default::default(),
    }
  }

  pub(crate) fn push_msg(&mut self, msg_id: MsgId, msg: Msg) {
    if cfg!(feature = "sync_verbose_log") {
      trace!("📩 queue: {}", msg);
    }

    self.queue.push(QueueItem::new(msg, msg_id));
  }
}

impl<Msg> Deref for SinkQueue<Msg>
where
  Msg: SinkMessage,
{
  type Target = BinaryHeap<QueueItem<Msg>>;

  fn deref(&self) -> &Self::Target {
    &self.queue
  }
}

impl<Msg> DerefMut for SinkQueue<Msg>
where
  Msg: SinkMessage,
{
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.queue
  }
}

#[derive(Debug, Clone)]
pub(crate) struct QueueItem<Msg> {
  inner: Msg,
  msg_id: MsgId,
}

impl<Msg> QueueItem<Msg>
where
  Msg: SinkMessage,
{
  pub fn new(msg: Msg, msg_id: MsgId) -> Self {
    Self { inner: msg, msg_id }
  }

  pub fn message(&self) -> &Msg {
    &self.inner
  }

  pub fn into_message(self) -> Msg {
    self.inner
  }

  pub fn msg_id(&self) -> MsgId {
    self.msg_id
  }
}

impl<Msg> QueueItem<Msg>
where
  Msg: SinkMessage,
{
  pub fn mergeable(&self) -> bool {
    self.inner.mergeable()
  }

  pub fn merge(&mut self, other: &Self, max_size: &usize) -> Result<bool, Error> {
    self.inner.merge(other.message(), max_size)
  }
}

impl<Msg> Eq for QueueItem<Msg> where Msg: Eq {}

impl<Msg> PartialEq for QueueItem<Msg>
where
  Msg: PartialEq,
{
  fn eq(&self, other: &Self) -> bool {
    self.inner == other.inner
  }
}

impl<Msg> PartialOrd for QueueItem<Msg>
where
  Msg: PartialOrd + Ord,
{
  fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
    Some(self.cmp(other))
  }
}

impl<Msg> Ord for QueueItem<Msg>
where
  Msg: Ord,
{
  fn cmp(&self, other: &Self) -> std::cmp::Ordering {
    self.inner.cmp(&other.inner)
  }
}
