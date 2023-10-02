use collab::core::origin::CollabOrigin;
use collab_define::collab_msg::CollabSinkMessage;

use std::marker::PhantomData;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::collab_sync::pending_msg::{MessageState, PendingMsgQueue};
use crate::collab_sync::{SyncError, DEFAULT_SYNC_TIMEOUT};
use futures_util::SinkExt;

use tokio::spawn;
use tokio::sync::{mpsc, oneshot, watch, Mutex};
use tokio::time::{interval, Instant, Interval};
use tracing::{debug, error, trace, warn};

#[derive(Clone, Debug)]
pub enum SinkState {
  Init,
  /// The sink is syncing the messages to the remote.
  Syncing,
  /// All the messages are synced to the remote.
  Finished,
}

impl SinkState {
  pub fn is_init(&self) -> bool {
    matches!(self, SinkState::Init)
  }
}

/// Use to sync the [Msg] to the remote.
pub struct CollabSink<Sink, Msg> {
  uid: i64,
  /// The [Sink] is used to send the messages to the remote. It might be a websocket sink or
  /// other sink that implements the [SinkExt] trait.
  sender: Arc<Mutex<Sink>>,

  /// The [PendingMsgQueue] is used to queue the messages that are waiting to be sent to the
  /// remote. It will merge the messages if possible.
  pending_msg_queue: Arc<parking_lot::Mutex<PendingMsgQueue<Msg>>>,
  msg_id_counter: Arc<dyn MsgIdCounter>,

  /// The [watch::Sender] is used to notify the [CollabSinkRunner] to process the pending messages.
  /// Sending `false` will stop the [CollabSinkRunner].
  notifier: Arc<watch::Sender<bool>>,
  config: SinkConfig,

  /// Stop the [IntervalRunner] if the sink strategy is [SinkStrategy::FixInterval].
  #[allow(dead_code)]
  interval_runner_stop_tx: Option<mpsc::Sender<()>>,

  /// Used to calculate the time interval between two messages. Only used when the sink strategy
  /// is [SinkStrategy::FixInterval].
  instant: Mutex<Instant>,
  state_notifier: Arc<watch::Sender<SinkState>>,
}

impl<Sink, Msg> Drop for CollabSink<Sink, Msg> {
  fn drop(&mut self) {
    let _ = self.notifier.send(true);
  }
}

impl<E, Sink, Msg> CollabSink<Sink, Msg>
where
  E: std::error::Error + Send + Sync + 'static,
  Sink: SinkExt<Msg, Error = E> + Send + Sync + Unpin + 'static,
  Msg: CollabSinkMessage,
{
  pub fn new<C>(
    uid: i64,
    sink: Sink,
    notifier: watch::Sender<bool>,
    sync_state_tx: watch::Sender<SinkState>,
    msg_id_counter: C,
    config: SinkConfig,
  ) -> Self
  where
    C: MsgIdCounter,
  {
    let notifier = Arc::new(notifier);
    let state_notifier = Arc::new(sync_state_tx);
    let sender = Arc::new(Mutex::new(sink));
    let pending_msg_queue = PendingMsgQueue::new(uid);
    let pending_msg_queue = Arc::new(parking_lot::Mutex::new(pending_msg_queue));
    let msg_id_counter = Arc::new(msg_id_counter);
    //
    let instant = Mutex::new(Instant::now());
    let mut interval_runner_stop_tx = None;
    if let SinkStrategy::FixInterval(duration) = &config.strategy {
      let weak_notifier = Arc::downgrade(&notifier);
      let (tx, rx) = mpsc::channel(1);
      interval_runner_stop_tx = Some(tx);
      spawn(IntervalRunner::new(*duration).run(weak_notifier, rx));
    }
    Self {
      uid,
      sender,
      pending_msg_queue,
      msg_id_counter,
      notifier,
      state_notifier,
      config,
      instant,
      interval_runner_stop_tx,
    }
  }

  /// Put the message into the queue and notify the sink to process the next message.
  /// After the [Msg] was pushed into the [PendingMsgQueue]. The queue will pop the next msg base on
  /// its priority. And the message priority is determined by the [Msg] that implement the [Ord] and
  /// [PartialOrd] trait. Check out the [CollabMessage] for more details.
  ///
  pub fn queue_msg(&self, f: impl FnOnce(MsgId) -> Msg) {
    {
      let mut pending_msg_queue = self.pending_msg_queue.lock();
      let msg_id = self.msg_id_counter.next();
      let msg = f(msg_id);
      pending_msg_queue.push_msg(msg_id, msg);
      drop(pending_msg_queue);
    }

    self.notify();
  }

  pub fn queue_init_sync(&self, f: impl FnOnce(MsgId) -> Msg) {
    // When the client is connected, remove all pending messages and send the init message.
    {
      let mut pending_msg_queue = self.pending_msg_queue.lock();
      pending_msg_queue.clear();

      let msg_id = self.msg_id_counter.next();
      let msg = f(msg_id);
      pending_msg_queue.push_msg(msg_id, msg);
      drop(pending_msg_queue);
    }

    self.notify();
  }

  pub fn clear(&self) {
    self.pending_msg_queue.lock().clear();
  }

  /// Notify the sink to process the next message and mark the current message as done.
  pub async fn ack_msg(
    &self,
    _origin: Option<&CollabOrigin>,
    object_id: &str,
    msg_id: MsgId,
  ) -> bool {
    match self.pending_msg_queue.lock().peek_mut() {
      None => false,
      Some(mut pending_msg) => {
        // In most cases, the msg_id of the pending_msg is the same as the passed-in msg_id. However,
        // due to network issues, the client might send multiple messages with the same msg_id.
        // Therefore, the msg_id might not always match the msg_id of the pending_msg.
        if pending_msg.msg_id() != msg_id {
          return false;
        }

        let is_done = pending_msg.set_state(self.uid, MessageState::Done);
        if is_done {
          trace!("[Client {}]: Did send: {}:{}", self.uid, object_id, msg_id);
          self.notify();
        }
        is_done
      },
    }
  }

  async fn process_next_msg(&self) -> Result<(), SyncError> {
    // Check if the next message can be deferred. If not, try to send the message immediately. The
    // default value is true.
    let deferrable = self
      .pending_msg_queue
      .try_lock()
      .map(|pending_msgs| {
        pending_msgs
          .peek()
          .map(|msg| msg.get_msg().deferrable())
          .unwrap_or(true)
      })
      .unwrap_or(true);

    if !deferrable {
      self.try_send_msg_immediately().await;
      return Ok(());
    }

    // Check the elapsed time from the last message. Return if the elapsed time is less than
    // the fix interval.
    if let SinkStrategy::FixInterval(duration) = &self.config.strategy {
      let elapsed = self.instant.lock().await.elapsed();
      // trace!(
      //   "elapsed interval: {:?}, fix interval: {:?}",
      //   elapsed,
      //   duration
      // );
      if elapsed < *duration {
        return Ok(());
      }
    }

    // Reset the instant if the strategy is [SinkStrategy::FixInterval].
    if self.config.strategy.is_fix_interval() {
      *self.instant.lock().await = Instant::now();
    }

    self.try_send_msg_immediately().await;
    Ok(())
  }

  async fn try_send_msg_immediately(&self) -> Option<()> {
    let (tx, rx) = oneshot::channel();
    let collab_msg = {
      let (mut pending_msg_queue, mut sending_msg) = match self.pending_msg_queue.try_lock() {
        None => {
          // If acquire the lock failed, try to notify again after 100ms
          retry_later(Arc::downgrade(&self.notifier));
          None
        },
        Some(mut pending_msg_queue) => pending_msg_queue
          .pop()
          .map(|sending_msg| (pending_msg_queue, sending_msg)),
      }?;
      if sending_msg.state().is_done() {
        // Notify to process the next pending message
        self.notify();
        return None;
      }

      // Do nothing if the message is still processing.
      if sending_msg.state().is_processing() {
        return None;
      }

      // If the message can merge other messages, try to merge the next message until the
      // message is not mergeable.
      if sending_msg.is_mergeable() {
        while let Some(pending_msg) = pending_msg_queue.pop() {
          debug!("merge_msg: {}", pending_msg.get_msg());
          sending_msg.merge(pending_msg);
          if !sending_msg.is_mergeable() {
            break;
          }
        }
      }

      sending_msg.set_ret(tx);
      sending_msg.set_state(self.uid, MessageState::Processing);
      let _ = self.state_notifier.send(SinkState::Syncing);
      let collab_msg = sending_msg.get_msg().clone();
      pending_msg_queue.push(sending_msg);
      collab_msg
    };

    match self.sender.try_lock() {
      Ok(mut sender) => {
        debug!("[Client {}] sending {}", self.uid, collab_msg);
        sender.send(collab_msg).await.ok()?;
      },
      Err(_) => {
        warn!("Failed to acquire the lock of the sink, retry later");
        retry_later(Arc::downgrade(&self.notifier));
        return None;
      },
    }

    // Wait for the message to be acked.
    // If the message is not acked within the timeout, resend the message.
    match tokio::time::timeout(self.config.timeout, rx).await {
      Ok(result) => {
        match result {
          Ok(_) => match self.pending_msg_queue.try_lock() {
            None => warn!("Failed to acquire the lock of the pending_msg_queue"),
            Some(mut pending_msg_queue) => {
              let _ = pending_msg_queue.pop();
              trace!("Pending messages: {}", pending_msg_queue.len());
              if pending_msg_queue.is_empty() {
                if let Err(e) = self.state_notifier.send(SinkState::Finished) {
                  error!("send sink state failed: {}", e);
                }
              }
            },
          },
          Err(err) => warn!("Send message failed error: {:?}", err),
        }

        self.notify()
      },
      Err(_) => {
        if let Some(mut pending_msg) = self.pending_msg_queue.lock().peek_mut() {
          pending_msg.set_state(self.uid, MessageState::Timeout);
        }
        self.notify();
      },
    }
    None
  }

  /// Notify the sink to process the next message.
  pub(crate) fn notify(&self) {
    let _ = self.notifier.send(false);
  }

  /// Stop the sink.
  #[allow(dead_code)]
  fn stop(&self) {
    let _ = self.notifier.send(true);
  }
}

fn retry_later(weak_notifier: Weak<watch::Sender<bool>>) {
  spawn(async move {
    interval(Duration::from_millis(100)).tick().await;
    if let Some(notifier) = weak_notifier.upgrade() {
      let _ = notifier.send(false);
    }
  });
}

pub struct CollabSinkRunner<Msg>(PhantomData<Msg>);

impl<Msg> CollabSinkRunner<Msg> {
  /// The runner will stop if the [CollabSink] was dropped or the notifier was closed.
  pub async fn run<E, Sink>(
    weak_sink: Weak<CollabSink<Sink, Msg>>,
    mut notifier: watch::Receiver<bool>,
  ) where
    E: std::error::Error + Send + Sync + 'static,
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

      // stops the runner if the value of notifier is `true`
      if *notifier.borrow() {
        break;
      }

      if let Some(sync_sink) = weak_sink.upgrade() {
        let _ = sync_sink.process_next_msg().await;
      } else {
        break;
      }
    }
  }
}

pub struct SinkConfig {
  /// `timeout` is the time to wait for the remote to ack the message. If the remote
  /// does not ack the message in time, the message will be sent again.
  pub timeout: Duration,
  /// `mergeable` indicates whether the messages are mergeable. If the messages are
  /// mergeable, the sink will try to merge the messages before sending them.
  pub mergeable: bool,
  /// `max_zip_size` is the maximum size of the messages to be merged.
  pub max_merge_size: usize,
  /// `strategy` is the strategy to send the messages.
  pub strategy: SinkStrategy,
}

impl SinkConfig {
  pub fn new() -> Self {
    Self::default()
  }
  pub fn with_timeout(mut self, secs: u64) -> Self {
    let timeout_duration = Duration::from_secs(secs);
    if let SinkStrategy::FixInterval(duration) = self.strategy {
      if timeout_duration < duration {
        tracing::warn!("The timeout duration should greater than the fix interval duration");
      }
    }
    self.timeout = timeout_duration;
    self
  }

  /// `mergeable` indicates whether the messages are mergeable. If the messages are
  /// mergeable, the sink will try to merge the messages before sending them.
  pub fn with_mergeable(mut self, mergeable: bool) -> Self {
    self.mergeable = mergeable;
    self
  }

  /// `max_zip_size` is the maximum size of the messages to be merged.
  pub fn with_max_merge_size(mut self, max_merge_size: usize) -> Self {
    self.max_merge_size = max_merge_size;
    self
  }

  pub fn with_strategy(mut self, strategy: SinkStrategy) -> Self {
    if let SinkStrategy::FixInterval(duration) = strategy {
      if self.timeout < duration {
        tracing::warn!("The timeout duration should greater than the fix interval duration");
      }
    }
    self.strategy = strategy;
    self
  }
}

impl Default for SinkConfig {
  fn default() -> Self {
    Self {
      timeout: Duration::from_secs(DEFAULT_SYNC_TIMEOUT),
      mergeable: false,
      max_merge_size: 4096,
      strategy: SinkStrategy::ASAP,
    }
  }
}

pub enum SinkStrategy {
  /// Send the message as soon as possible.
  ASAP,
  /// Send the message in a fixed interval.
  /// This can reduce the number of times the message is sent. Especially if using the AWS
  /// as the storage layer, the cost of sending the message is high. However, it may increase
  /// the latency of the message.
  FixInterval(Duration),
}

impl SinkStrategy {
  pub fn is_fix_interval(&self) -> bool {
    matches!(self, SinkStrategy::FixInterval(_))
  }
}

pub type MsgId = u64;

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
}

impl MsgIdCounter for DefaultMsgIdCounter {
  fn next(&self) -> MsgId {
    self.0.fetch_add(1, Ordering::SeqCst)
  }
}

struct IntervalRunner {
  interval: Option<Interval>,
}

impl IntervalRunner {
  fn new(duration: Duration) -> Self {
    Self {
      interval: Some(tokio::time::interval(duration)),
    }
  }
}

impl IntervalRunner {
  pub async fn run(mut self, sender: Weak<watch::Sender<bool>>, mut stop_rx: mpsc::Receiver<()>) {
    let mut interval = self
      .interval
      .take()
      .expect("Interval should only take once");
    loop {
      tokio::select! {
        _ = stop_rx.recv() => {
            break;
        },
        _ = interval.tick() => {
          if let Some(sender) = sender.upgrade() {
            let _ = sender.send(false);
          } else {
            break;
          }
        }
      }
    }
  }
}
