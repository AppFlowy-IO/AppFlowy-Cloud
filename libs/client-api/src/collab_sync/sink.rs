use crate::af_spawn;
use crate::collab_sync::sink_config::SinkConfig;
use crate::collab_sync::sink_queue::{QueueItem, SinkQueue};
use crate::collab_sync::SyncObject;
use futures_util::SinkExt;

use realtime_entity::collab_msg::{CollabSinkMessage, MsgId, ServerCollabMessage};
use std::collections::{HashMap, HashSet};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::sync::{watch, Mutex};
use tokio::time::{interval, sleep};
use tracing::{error, trace, warn};

#[derive(Clone, Debug)]
pub enum SinkState {
  Init,
  /// The sink is syncing the messages to the remote.
  Syncing,
  /// All the messages are synced to the remote.
  Finished,
  Pause,
}

impl SinkState {
  pub fn is_init(&self) -> bool {
    matches!(self, SinkState::Init)
  }

  pub fn is_syncing(&self) -> bool {
    matches!(self, SinkState::Syncing)
  }
}

#[derive(Clone)]
pub enum SinkSignal {
  Stop,
  Proceed,
  ProcessAfterMillis(u64),
}

const SEND_INTERVAL: Duration = Duration::from_secs(8);

pub const COLLAB_SINK_DELAY_MILLIS: u64 = 500;

/// Use to sync the [Msg] to the remote.
pub struct CollabSink<Sink, Msg> {
  #[allow(dead_code)]
  uid: i64,
  /// The [Sink] is used to send the messages to the remote. It might be a websocket sink or
  /// other sink that implements the [SinkExt] trait.
  sender: Arc<Mutex<Sink>>,
  /// The [SinkQueue] is used to queue the messages that are waiting to be sent to the
  /// remote. It will merge the messages if possible.
  message_queue: Arc<parking_lot::Mutex<SinkQueue<Msg>>>,
  msg_id_counter: Arc<DefaultMsgIdCounter>,
  /// The [watch::Sender] is used to notify the [CollabSinkRunner] to process the pending messages.
  /// Sending `false` will stop the [CollabSinkRunner].
  notifier: Arc<watch::Sender<SinkSignal>>,
  config: SinkConfig,
  state_notifier: Arc<watch::Sender<SinkState>>,
  pause: AtomicBool,
  object: SyncObject,
  flying_messages: Arc<parking_lot::Mutex<HashSet<MsgId>>>,
}

impl<Sink, Msg> Drop for CollabSink<Sink, Msg> {
  fn drop(&mut self) {
    trace!("Drop CollabSink {}", self.object.object_id);
    let _ = self.notifier.send(SinkSignal::Stop);
  }
}

impl<E, Sink, Msg> CollabSink<Sink, Msg>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<Vec<Msg>, Error = E> + Send + Sync + Unpin + 'static,
  Msg: CollabSinkMessage,
{
  pub fn new(
    uid: i64,
    object: SyncObject,
    sink: Sink,
    notifier: watch::Sender<SinkSignal>,
    sync_state_tx: watch::Sender<SinkState>,
    config: SinkConfig,
    pause: bool,
  ) -> Self {
    let msg_id_counter = DefaultMsgIdCounter::new();
    let notifier = Arc::new(notifier);
    let state_notifier = Arc::new(sync_state_tx);
    let sender = Arc::new(Mutex::new(sink));
    let msg_queue = SinkQueue::new(uid);
    let message_queue = Arc::new(parking_lot::Mutex::new(msg_queue));
    let msg_id_counter = Arc::new(msg_id_counter);
    let flying_messages = Arc::new(parking_lot::Mutex::new(HashSet::new()));

    let mut interval = interval(SEND_INTERVAL);
    let weak_notifier = Arc::downgrade(&notifier);
    let weak_flying_messages = Arc::downgrade(&flying_messages);
    af_spawn(async move {
      loop {
        interval.tick().await;
        match weak_notifier.upgrade() {
          Some(notifier) => {
            // Removing the flying messages allows for the re-sending of the top k messages in the message queue.
            if let Some(flying_messages) = weak_flying_messages.upgrade() {
              if let Some(mut flying_messages) = flying_messages.try_lock() {
                flying_messages.clear();
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
      sender,
      message_queue,
      msg_id_counter,
      notifier,
      state_notifier,
      config,
      pause: AtomicBool::new(pause),
      object,
      flying_messages,
    }
  }

  /// Put the message into the queue and notify the sink to process the next message.
  /// After the [Msg] was pushed into the [SinkQueue]. The queue will pop the next msg base on
  /// its priority. And the message priority is determined by the [Msg] that implement the [Ord] and
  /// [PartialOrd] trait. Check out the [CollabMessage] for more details.
  ///
  pub fn queue_msg(&self, f: impl FnOnce(MsgId) -> Msg) {
    if !self.state_notifier.borrow().is_syncing() {
      let _ = self.state_notifier.send(SinkState::Syncing);
    }

    let mut msg_queue = self.message_queue.lock();
    let msg_id = self.msg_id_counter.next();
    let new_msg = f(msg_id);

    let mut requeue_items = Vec::with_capacity(msg_queue.len());
    if new_msg.is_server_init_sync() {
      // When the message is a server initialization sync, indicating the absence of a client
      // initialization sync in the queue, it means the server initialization sync contains the
      // [Collab]'s latest updates. Therefore, we can clear the update sync messages in the queue.
      while let Some(item) = msg_queue.pop() {
        if !item.message().is_update_sync() {
          let msg_id = self.msg_id_counter.next();
          let mut msg = item.into_message();
          msg.set_msg_id(msg_id);
          requeue_items.push(QueueItem::new(msg, msg_id));
        }
      }
      self.flying_messages.lock().clear();
    }

    trace!("🔥 queue {}", new_msg);
    msg_queue.push_msg(msg_id, new_msg);
    if !requeue_items.is_empty() {
      trace!(
        "{} requeue items: {}",
        self.object.object_id,
        requeue_items
          .iter()
          .map(|item| { item.msg_id().to_string() })
          .collect::<Vec<_>>()
          .join(",")
      );
      msg_queue.extend(requeue_items);
    }
    drop(msg_queue);
    // Notify the sink to process the next message after 500ms.
    let _ = self
      .notifier
      .send(SinkSignal::ProcessAfterMillis(COLLAB_SINK_DELAY_MILLIS));

    self.merge();
  }

  /// When queue the init message, the sink will clear all the pending messages and send the init
  /// message immediately.
  pub fn queue_init_sync(&self, f: impl FnOnce(MsgId) -> Msg) {
    if !self.state_notifier.borrow().is_syncing() {
      let _ = self.state_notifier.send(SinkState::Syncing);
    }

    // Clear all the pending messages and send the init message immediately.
    self.clear();

    // When the client is connected, remove all pending messages and send the init message.
    let mut msg_queue = self.message_queue.lock();
    let msg_id = self.msg_id_counter.next();
    msg_queue.push_msg(msg_id, f(msg_id));
    let _ = self.notifier.send(SinkSignal::Proceed);
  }

  pub fn can_queue_init_sync(&self) -> bool {
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
    match self.flying_messages.try_lock() {
      None => error!("failed to acquire the lock of the flying message"),
      Some(mut flying_messages) => {
        flying_messages.clear();
      },
    }
  }

  pub fn pause(&self) {
    self.pause.store(true, Ordering::SeqCst);
    let _ = self.state_notifier.send(SinkState::Pause);
  }

  pub fn resume(&self) {
    self.pause.store(false, Ordering::SeqCst);
    self.notify();
  }

  /// Notify the sink to process the next message and mark the current message as done.
  /// Returns bool value to indicate whether the message is valid.
  pub async fn validate_server_message(&self, server_message: &ServerCollabMessage) -> bool {
    if server_message.msg_id().is_none() {
      // msg_id will be None for [ServerBroadcast] or [ServerAwareness], automatically valid.
      return true;
    }

    // safety: msg_id is not None
    let income_message_id = server_message.msg_id().unwrap();
    let mut flying_messages = self.flying_messages.lock();

    // if the message id is not in the flying messages, it means the message is invalid.
    if !flying_messages.contains(&income_message_id) {
      return false;
    }

    let mut message_queue = self.message_queue.lock();
    let mut is_valid = false;
    // if flying_messages.contains(&income_message_id) {
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
        flying_messages.remove(&income_message_id);
      }
    }

    trace!(
      "{:?}: Pending messages:{} ids:{}",
      self.object.object_id,
      message_queue.len(),
      message_queue
        .iter()
        .map(|item| item.msg_id().to_string())
        .collect::<Vec<_>>()
        .join(",")
    );

    if message_queue.is_empty() {
      if let Err(e) = self.state_notifier.send(SinkState::Finished) {
        error!("send sink state failed: {}", e);
      }
    }
    is_valid
  }

  async fn process_next_msg(&self) {
    if self.pause.load(Ordering::SeqCst) {
      return;
    }
    self.send_msg_immediately().await;
  }

  async fn send_msg_immediately(&self) {
    let items = {
      let mut msg_queue = match self.message_queue.try_lock() {
        None => {
          // If acquire the lock failed, try later
          retry_later(Arc::downgrade(&self.notifier));
          return;
        },
        Some(msg_queue) => msg_queue,
      };
      let mut flying_messages = self.flying_messages.lock();
      let mut items = vec![];

      let mut count = 0;
      let mut item_to_requeue = vec![];
      while let Some(item) = msg_queue.pop() {
        if count > 20 {
          item_to_requeue.push(item);
          break;
        }

        if flying_messages.contains(&item.msg_id()) {
          item_to_requeue.push(item);
          continue;
        }

        let is_init_sync = item.message().is_client_init_sync();
        items.push(item.clone());
        count += 1;
        item_to_requeue.push(item);

        if is_init_sync {
          break;
        }
      }
      msg_queue.extend(item_to_requeue);

      let message_ids = items.iter().map(|item| item.msg_id()).collect::<Vec<_>>();
      flying_messages.extend(message_ids);

      items
    };

    if items.is_empty() {
      return;
    }

    let message_ids = items.iter().map(|item| item.msg_id()).collect::<Vec<_>>();
    let messages = items
      .into_iter()
      .map(|item| item.into_message())
      .collect::<Vec<_>>();
    match self.sender.try_lock() {
      Ok(mut sender) => match sender.send(messages).await {
        Ok(_) => {
          trace!(
            "🔥 sending {} messages {:?}",
            self.object.object_id,
            message_ids
          );
        },
        Err(err) => {
          error!("Failed to send error: {:?}", err.into());
          self
            .flying_messages
            .lock()
            .retain(|id| !message_ids.contains(id));
        },
      },
      Err(_) => {
        warn!("failed to acquire the lock of the sink, retry later");
        self
          .flying_messages
          .lock()
          .retain(|id| !message_ids.contains(id));
        retry_later(Arc::downgrade(&self.notifier));
      },
    }
  }

  fn merge(&self) {
    if self.config.disable_merge_message {
      return;
    }

    if let (Some(flying_messages), Some(mut msg_queue)) = (
      self.flying_messages.try_lock(),
      self.message_queue.try_lock(),
    ) {
      let mut items: Vec<QueueItem<Msg>> = Vec::with_capacity(msg_queue.len());
      let mut merged_ids = HashMap::new();
      while let Some(next) = msg_queue.pop() {
        // If the message is in the flying messages, it means the message is sending to the remote.
        // So don't merge the message.
        if flying_messages.contains(&next.msg_id()) {
          items.push(next);
          continue;
        }

        // Try to merge the next message with the last message. Only merge when:
        // 1. The last message is not in the flying messages.
        // 2. The last message can be merged.
        // 3. The last message's payload size is less than the maximum payload size.
        if let Some(last) = items.last_mut() {
          if !flying_messages.contains(&last.msg_id())
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

      if cfg!(debug_assertions) {
        for (msg_id, merged_ids) in merged_ids {
          trace!("merged {:?} messages into: {:?}", merged_ids, msg_id);
        }
      }
      msg_queue.extend(items);
    }
  }

  /// Notify the sink to process the next message.
  pub(crate) fn notify(&self) {
    let _ = self.notifier.send(SinkSignal::Proceed);
  }
}

fn retry_later(weak_notifier: Weak<watch::Sender<SinkSignal>>) {
  af_spawn(async move {
    interval(Duration::from_millis(300)).tick().await;
    if let Some(notifier) = weak_notifier.upgrade() {
      let _ = notifier.send(SinkSignal::Proceed);
    }
  });
}

pub struct CollabSinkRunner<Msg>(PhantomData<Msg>);

impl<Msg> CollabSinkRunner<Msg> {
  /// The runner will stop if the [CollabSink] was dropped or the notifier was closed.
  pub async fn run<E, Sink>(
    weak_sink: Weak<CollabSink<Sink, Msg>>,
    mut notifier: watch::Receiver<SinkSignal>,
  ) where
    E: Into<anyhow::Error> + Send + Sync + 'static,
    Sink: SinkExt<Vec<Msg>, Error = E> + Send + Sync + Unpin + 'static,
    Msg: CollabSinkMessage,
  {
    if let Some(sink) = weak_sink.upgrade() {
      sink.notify();
    }
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
  fn next(&self) -> MsgId {
    self.0.fetch_add(1, Ordering::SeqCst)
  }
}
