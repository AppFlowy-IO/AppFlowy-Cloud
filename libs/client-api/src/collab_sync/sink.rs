use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::collab_sync::sink_queue::{MessageState, QueueItem, SinkQueue};
use crate::collab_sync::SyncObject;
use futures_util::SinkExt;

use crate::af_spawn;
use crate::collab_sync::sink_config::SinkConfig;
use realtime_entity::collab_msg::{CollabSinkMessage, MsgId, ServerCollabMessage};

use tokio::sync::{watch, Mutex};

use tokio::time::{interval, sleep};
use tracing::{debug, error, trace, warn};

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
  Sink: SinkExt<Msg, Error = E> + Send + Sync + Unpin + 'static,
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

    let mut interval = interval(SEND_INTERVAL);
    let weak_notifier = Arc::downgrade(&notifier);
    af_spawn(async move {
      loop {
        interval.tick().await;
        match weak_notifier.upgrade() {
          Some(notifier) => {
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
    let msg = f(msg_id);
    msg_queue.push_msg(msg_id, msg);

    // If the next message can be merged, try to merge the next message.
    if let Some(mut next) = msg_queue.pop() {
      self.merge_items_into_next(&mut next, &mut msg_queue);
      msg_queue.push(next);
    }
    drop(msg_queue);

    // Notify the sink to process the next message after 500ms.
    let _ = self
      .notifier
      .send(SinkSignal::ProcessAfterMillis(COLLAB_SINK_DELAY_MILLIS));
  }

  /// When queue the init message, the sink will clear all the pending messages and send the init
  /// message immediately.
  pub fn queue_init_sync(&self, f: impl FnOnce(MsgId) -> Msg) {
    if !self.state_notifier.borrow().is_syncing() {
      let _ = self.state_notifier.send(SinkState::Syncing);
    }

    // When the client is connected, remove all pending messages and send the init message.
    {
      let mut msg_queue = self.message_queue.lock();
      msg_queue.clear();
      let msg_id = self.msg_id_counter.next();
      let msg = f(msg_id);
      msg_queue.push_msg(msg_id, msg);
      drop(msg_queue);
    }

    let _ = self.notifier.send(SinkSignal::Proceed);
  }

  pub fn can_queue_init_sync(&self) -> bool {
    let msg_queue = self.message_queue.lock();
    if let Some(msg) = msg_queue.peek() {
      if msg.get_msg().is_init_msg() {
        return false;
      }
    }
    true
  }

  pub fn clear(&self) {
    self.message_queue.lock().clear();
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
  pub async fn ack_msg(&self, msg: &ServerCollabMessage) -> bool {
    if msg.msg_id().is_none() {
      // msg_id will be None for [ServerBroadcast] or [ServerAwareness], automatically valid.
      return true;
    }

    // safety: msg_id is not None
    let msg_id = msg.msg_id().unwrap();
    let mut is_valid = false;
    {
      let mut lock_guard = self.message_queue.lock();
      if let Some(current_item) = lock_guard.pop() {
        // In most cases, the msg_id of the pending_msg is the same as the passed-in msg_id. However,
        // due to network issues, the client might send multiple messages with the same msg_id.
        // Therefore, the msg_id might not always match the msg_id of the pending_msg.
        if current_item.msg_id() != msg_id {
          lock_guard.push(current_item);
        }

        trace!(
          "{:?}: Pending message len: {}",
          self.object.object_id,
          lock_guard.len()
        );

        if lock_guard.is_empty() {
          if let Err(e) = self.state_notifier.send(SinkState::Finished) {
            error!("send sink state failed: {}", e);
          }
        }
        is_valid = true;
      }
    }
    // If the message is valid, notify the sink to process the next message.
    if is_valid {
      self.process_next_msg().await;
    }
    is_valid
  }

  async fn process_next_msg(&self) {
    if self.pause.load(Ordering::SeqCst) {
      return;
    }
    self.send_msg_immediately().await;
  }

  async fn send_msg_immediately(&self) -> Option<()> {
    let collab_msg = {
      let (mut msg_queue, mut next) = match self.message_queue.try_lock() {
        None => {
          // If acquire the lock failed, try to notify again after 100ms
          retry_later(Arc::downgrade(&self.notifier));
          None
        },
        Some(mut msg_queue) => msg_queue.pop().map(|sending_msg| (msg_queue, sending_msg)),
      }?;

      //
      if next.is_processing() {
        msg_queue.push(next);
        return None;
      }

      self.merge_items_into_next(&mut next, &mut msg_queue);
      next.set_state(MessageState::Processing);
      let collab_msg = next.get_msg().clone();
      msg_queue.push(next);
      collab_msg
    };

    match self.sender.try_lock() {
      Ok(mut sender) => {
        debug!("Sending {}", collab_msg);
        if let Err(err) = sender.send(collab_msg).await {
          error!("Failed to send error: {:?}", err.into());
        }
      },
      Err(_) => {
        warn!("Failed to acquire the lock of the sink, retry later");
        retry_later(Arc::downgrade(&self.notifier));
      },
    }

    None
  }

  /// Merge the next message with the subsequent messages if possible.
  fn merge_items_into_next(&self, next: &mut QueueItem<Msg>, msg_queue: &mut SinkQueue<Msg>) {
    if self.config.disable_merge_message {
      return;
    }

    // Proceed only if the next message is eligible for merging.
    if !next.can_merge() {
      return;
    }

    // Attempt to merge subsequent messages until it's no longer possible or advisable.
    while let Some(pending_msg) = msg_queue.pop() {
      // Attempt to merge the current message with the next one.
      // If merging is not successful or not advisable, push the unmerged message back and exit.
      match next.merge(&pending_msg, &self.config.maximum_payload_size) {
        Ok(success) => {
          if !success {
            msg_queue.push(pending_msg);
            break;
          }
        },
        Err(err) => {
          msg_queue.push(pending_msg); // Requeue the message that couldn't be merged.
          error!("Failed to merge message: {}", err);
          break;
        },
      }
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
    Sink: SinkExt<Msg, Error = E> + Send + Sync + Unpin + 'static,
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
