use crate::metrics::CollabStreamMetrics;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use futures::{Stream, StreamExt};
use loole::{Receiver, Sender};
use redis::streams::{StreamReadOptions, StreamReadReply};
use redis::Client;
use redis::Commands;
use redis::Connection;
use redis::RedisError;
use redis::RedisResult;
use redis::Value;
use std::collections::HashMap;
use std::fmt::Display;
use std::pin::Pin;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::thread::{sleep, JoinHandle};
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;

/// Redis stream key.
pub type StreamKey = String;

pub trait FromRedisStream {
  type Error: Display;
  fn from_redis_stream(msg_id: &str, fields: &RedisMap) -> Result<Self, Self::Error>
  where
    Self: Sized;
}

/// Channel returned by [StreamRouter::observe], that allows to receive messages retrieved by
/// the router.
pub struct StreamReader<T> {
  receiver: BroadcastStream<Arc<(String, RedisMap)>>,
  _phantom: std::marker::PhantomData<T>,
}

impl<T> StreamReader<T> {
  pub fn new(receiver: StreamReceiver) -> Self {
    Self {
      receiver: BroadcastStream::new(receiver),
      _phantom: std::marker::PhantomData,
    }
  }
}

impl<T> Stream for StreamReader<T>
where
  T: FromRedisStream,
{
  type Item = Result<T, T::Error>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let pin = unsafe { self.map_unchecked_mut(|v| &mut v.receiver) };
    match pin.poll_next(cx) {
      Poll::Ready(Some(Ok(msg))) => {
        let result = T::from_redis_stream(&msg.0, &msg.1);
        Poll::Ready(Some(result))
      },
      Poll::Ready(Some(Err(err))) => {
        tracing::error!("failed to receive message: {}", err);
        Poll::Ready(None)
      },
      Poll::Ready(None) => Poll::Ready(None),
      Poll::Pending => Poll::Pending,
    }
  }
}

/// Redis stream router used to multiplex multiple number of Redis stream read requests over a
/// fixed number of Redis connections.
pub struct StreamRouter {
  buf: Sender<StreamHandle>,
  alive: Arc<AtomicBool>,
  #[allow(dead_code)]
  workers: Vec<Worker>,
  metrics: Arc<CollabStreamMetrics>,
  channels: DashMap<StreamKey, StreamSender>,
  buffer_capacity: usize,
}

impl StreamRouter {
  pub fn new(client: &Client, metrics: Arc<CollabStreamMetrics>) -> Result<Self, RedisError> {
    Self::with_options(client, metrics, Default::default())
  }

  pub fn with_options(
    client: &Client,
    metrics: Arc<CollabStreamMetrics>,
    options: StreamRouterOptions,
  ) -> Result<Self, RedisError> {
    let alive = Arc::new(AtomicBool::new(true));
    let (tx, rx) = loole::unbounded();
    let mut workers = Vec::with_capacity(options.worker_count);
    for worker_id in 0..options.worker_count {
      let conn = client.get_connection()?;
      let worker = Worker::new(
        worker_id,
        conn,
        tx.clone(),
        rx.clone(),
        alive.clone(),
        &options,
        metrics.clone(),
      );
      workers.push(worker);
    }
    tracing::info!("stared Redis stream router with {} workers", workers.len());
    Ok(Self {
      buf: tx,
      workers,
      alive,
      metrics,
      channels: DashMap::new(),
      buffer_capacity: options.xread_count.unwrap_or(10_000),
    })
  }

  pub fn observe<T: FromRedisStream>(
    &self,
    stream_key: StreamKey,
    last_id: Option<String>,
  ) -> StreamReader<T> {
    let rx = match self.channels.entry(stream_key.clone()) {
      Entry::Vacant(e) => {
        tracing::trace!("creating new stream channel for {}", e.key());
        let (tx, rx) = tokio::sync::broadcast::channel(self.buffer_capacity);
        let last_id = last_id.unwrap_or_else(|| "0".to_string());
        let h = StreamHandle::new(stream_key.clone(), last_id, tx.clone());
        self.buf.send(h).unwrap();
        self.metrics.reads_enqueued.inc();
        e.insert(tx);
        rx
      },
      Entry::Occupied(e) => {
        tracing::trace!("reusing existing stream channel for {}", e.key());
        e.get().subscribe()
      },
    };
    StreamReader::new(rx)
  }
}

impl Drop for StreamRouter {
  fn drop(&mut self) {
    self.alive.store(false, SeqCst);
  }
}

/// Options used to configure [StreamRouter].
#[derive(Debug, Clone)]
pub struct StreamRouterOptions {
  /// Number of worker threads. Each worker thread has its own Redis connection.
  /// Default: number of CPU threads but can vary under specific circumstances.
  pub worker_count: usize,
  /// How many Redis streams a single Redis poll worker can read at a time.
  /// Default: 100
  pub xread_streams: usize,
  /// How long poll worker will be blocked while waiting for Redis `XREAD` request to respond.
  /// This blocks a worker thread and doesn't affect other threads.
  ///
  /// If set to `None` it won't block and will return immediately, which gives a biggest
  /// responsiveness but can lead to unnecessary active loops causing CPU spikes even when idle.
  ///
  /// Default: `Some(0)` meaning blocking for indefinite amount of time.
  pub xread_block_millis: Option<usize>,
  /// How many messages a single worker's `XREAD` request is allowed to return.
  /// Default: `None` (unbounded).
  pub xread_count: Option<usize>,
}

impl Default for StreamRouterOptions {
  fn default() -> Self {
    StreamRouterOptions {
      worker_count: std::thread::available_parallelism().unwrap().get(),
      xread_streams: 100,
      xread_block_millis: Some(0),
      xread_count: None,
    }
  }
}

struct Worker {
  _handle: JoinHandle<()>,
}

impl Worker {
  fn new(
    worker_id: usize,
    conn: Connection,
    tx: Sender<StreamHandle>,
    rx: Receiver<StreamHandle>,
    alive: Arc<AtomicBool>,
    options: &StreamRouterOptions,
    metrics: Arc<CollabStreamMetrics>,
  ) -> Self {
    let mut xread_options = StreamReadOptions::default();
    if let Some(block_millis) = options.xread_block_millis {
      xread_options = xread_options.block(block_millis);
    }
    if let Some(count) = options.xread_count {
      xread_options = xread_options.count(count);
    }
    let count = options.xread_streams;
    let handle = std::thread::spawn(move || {
      if let Err(err) = Self::process_streams(conn, tx, rx, alive, xread_options, count, metrics) {
        tracing::error!("worker {} failed: {}", worker_id, err);
      }
    });
    Self { _handle: handle }
  }

  fn process_streams(
    mut conn: Connection,
    tx: Sender<StreamHandle>,
    rx: Receiver<StreamHandle>,
    alive: Arc<AtomicBool>,
    options: StreamReadOptions,
    count: usize,
    metrics: Arc<CollabStreamMetrics>,
  ) -> RedisResult<()> {
    let mut stream_keys = Vec::with_capacity(count);
    let mut message_ids = Vec::with_capacity(count);
    let mut senders = HashMap::with_capacity(count);
    while alive.load(SeqCst) {
      // receive next `count` of stream read requests
      if !Self::read_buf(&rx, &mut stream_keys, &mut message_ids, &mut senders) {
        break; // rx channel has closed
      }

      let key_count = stream_keys.len();
      if key_count == 0 {
        tracing::warn!("Bug: read empty buf");
        sleep(Duration::from_millis(100));
        continue;
      }

      metrics.reads_dequeued.inc_by(key_count as u64);
      let result: StreamReadReply = conn.xread_options(&stream_keys, &message_ids, &options)?;

      let mut msgs = 0;
      for stream in result.keys {
        // for each stream returned from Redis, resolve corresponding subscriber and send messages
        let mut remove_sender = false;
        if let Some((sender, idx)) = senders.get(stream.key.as_str()) {
          for id in stream.ids {
            let message_id = id.id;
            let value = id.map;
            message_ids[*idx].clone_from(&message_id); //TODO: optimize
            msgs += 1;
            if let Err(err) = sender.send(Arc::new((message_id, value))) {
              tracing::debug!("failed to send: {}", err);
              remove_sender = true;
            }
          }
        }

        if remove_sender {
          senders.remove(stream.key.as_str());
        }
      }

      if msgs > 0 {
        tracing::trace!(
          "XREAD: read total of {} messages for {} streams",
          msgs,
          key_count
        );
      }
      let scheduled = Self::schedule_back(&tx, &mut stream_keys, &mut message_ids, &mut senders);
      metrics.reads_enqueued.inc_by(scheduled as u64);
    }
    Ok(())
  }

  fn schedule_back(
    tx: &Sender<StreamHandle>,
    keys: &mut Vec<StreamKey>,
    ids: &mut Vec<String>,
    senders: &mut HashMap<&str, (StreamSender, usize)>,
  ) -> usize {
    let keys = keys.drain(..);
    let mut ids = ids.drain(..);
    let mut scheduled = 0;
    for key in keys {
      if let Some(last_id) = ids.next() {
        if let Some((sender, _)) = senders.remove(key.as_str()) {
          if sender.receiver_count() == 0 {
            continue; // sender is already closed
          }
          let h = StreamHandle::new(key, last_id, sender);
          if let Err(err) = tx.send(h) {
            tracing::error!("failed to reschedule: {}", err);
            break;
          }
          scheduled += 1;
        }
      }
    }
    senders.clear();
    scheduled
  }

  fn read_buf(
    rx: &Receiver<StreamHandle>,
    stream_keys: &mut Vec<StreamKey>,
    message_ids: &mut Vec<String>,
    senders: &mut HashMap<&'static str, (StreamSender, usize)>,
  ) -> bool {
    // try to receive first element - block thread if there's none
    let mut count = stream_keys.capacity();
    if let Ok(h) = rx.recv() {
      // senders and stream_keys have bound lifetimes and fixed internal buffers
      // since API users are using StreamKeys => String, we want to avoid allocations
      let key_ref: &'static str = unsafe { std::mem::transmute(h.key.as_str()) };
      senders.insert(key_ref, (h.sender, stream_keys.len()));
      stream_keys.push(h.key);
      message_ids.push(h.last_id.to_string());

      count -= 1;
      if count == 0 {
        return true;
      }

      // try to fill more without blocking if there's anything on the receiver
      while let Ok(h) = rx.try_recv() {
        let key_ref: &'static str = unsafe { std::mem::transmute(h.key.as_str()) };
        senders.insert(key_ref, (h.sender, stream_keys.len()));
        stream_keys.push(h.key);
        message_ids.push(h.last_id.to_string());

        count -= 1;
        if count == 0 {
          return true;
        }
      }
      true
    } else {
      false
    }
  }
}

pub type RedisMap = HashMap<String, Value>;
type StreamSender = tokio::sync::broadcast::Sender<Arc<(String, RedisMap)>>;
type StreamReceiver = tokio::sync::broadcast::Receiver<Arc<(String, RedisMap)>>;

struct StreamHandle {
  key: StreamKey,
  last_id: String,
  sender: StreamSender,
}

impl StreamHandle {
  fn new(key: StreamKey, last_id: String, sender: StreamSender) -> Self {
    StreamHandle {
      key,
      last_id,
      sender,
    }
  }
}

#[cfg(test)]
mod test {
  use crate::metrics::CollabStreamMetrics;
  use crate::stream_router::{FromRedisStream, RedisMap, StreamRouter, StreamRouterOptions};
  use futures::StreamExt;
  use rand::random;
  use redis::{Client, Commands, FromRedisValue};
  use std::sync::Arc;
  use tokio::task::JoinSet;

  struct TestMessage {
    id: String,
    data: String,
  }

  impl FromRedisStream for TestMessage {
    type Error = anyhow::Error;

    fn from_redis_stream(id: &str, fields: &RedisMap) -> Result<Self, Self::Error>
    where
      Self: Sized,
    {
      let data = fields
        .get("data")
        .ok_or_else(|| anyhow::anyhow!("expecting field `data`"))?;
      let data = String::from_redis_value(data)?;
      Ok(TestMessage {
        id: id.to_owned(),
        data,
      })
    }
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn multi_worker_preexisting_messages() {
    const ROUTES_COUNT: usize = 200;
    const MSG_PER_ROUTE: usize = 10;

    let mut client = Client::open("redis://127.0.0.1/").unwrap();
    let keys = init_streams(&mut client, ROUTES_COUNT, MSG_PER_ROUTE);
    let metrics = Arc::new(CollabStreamMetrics::default());

    let router = StreamRouter::new(&client, metrics).unwrap();

    let mut join_set = JoinSet::new();
    for key in keys {
      let mut observer = router.observe(key.clone(), None);
      join_set.spawn(async move {
        for i in 0..MSG_PER_ROUTE {
          let msg: TestMessage = observer.next().await.unwrap().unwrap();
          assert_eq!(msg.data, format!("{}-{}", key, i));
        }
      });
    }

    while let Some(t) = join_set.join_next().await {
      t.unwrap();
    }
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn multi_worker_live_messages() {
    const ROUTES_COUNT: usize = 200;
    const MSG_PER_ROUTE: usize = 10;
    let mut client = Client::open("redis://127.0.0.1/").unwrap();
    let keys = init_streams(&mut client, ROUTES_COUNT, 0);
    let metrics = Arc::new(CollabStreamMetrics::default());

    let router = StreamRouter::new(&client, metrics).unwrap();

    let mut join_set = JoinSet::new();
    for key in keys.iter() {
      let mut observer = router.observe(key.clone(), None);
      let key = key.clone();
      join_set.spawn(async move {
        for i in 0..MSG_PER_ROUTE {
          let msg: TestMessage = observer.next().await.unwrap().unwrap();
          assert_eq!(msg.data, format!("{}-{}", key, i));
        }
      });
    }

    for msg_idx in 0..MSG_PER_ROUTE {
      for key in keys.iter() {
        let data = format!("{}-{}", key, msg_idx);
        let _: String = client.xadd(key, "*", &[("data", data)]).unwrap();
      }
    }

    while let Some(t) = join_set.join_next().await {
      t.unwrap();
    }
  }

  #[tokio::test]
  async fn stream_reader_continue_from() {
    let mut client = Client::open("redis://127.0.0.1/").unwrap();
    let key = format!("test:{}:{}", random::<u32>(), 0);
    let _: String = client.xadd(&key, "*", &[("data", "1")]).unwrap();
    let m2: String = client.xadd(&key, "*", &[("data", "2")]).unwrap();
    let m3: String = client.xadd(&key, "*", &[("data", "3")]).unwrap();
    let metrics = Arc::new(CollabStreamMetrics::default());

    let router = StreamRouter::new(&client, metrics).unwrap();
    let mut observer = router.observe(key, Some(m2));

    let msg: TestMessage = observer.next().await.unwrap().unwrap();
    assert_eq!(msg.id, m3);
    assert_eq!(msg.data, "3");
  }

  #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
  async fn drop_subscription() {
    const ROUTES_COUNT: usize = 1;
    const MSG_PER_ROUTE: usize = 10;

    let mut client = Client::open("redis://127.0.0.1/").unwrap();
    let mut keys = init_streams(&mut client, ROUTES_COUNT, MSG_PER_ROUTE);
    let metrics = Arc::new(CollabStreamMetrics::default());

    let router = StreamRouter::with_options(
      &client,
      metrics.clone(),
      StreamRouterOptions {
        worker_count: 2,
        xread_streams: 100,
        xread_block_millis: Some(50),
        xread_count: None,
      },
    )
    .unwrap();

    let key = keys.pop().unwrap();
    let mut observer = router.observe(key.clone(), None);
    for i in 0..MSG_PER_ROUTE {
      let msg: TestMessage = observer.next().await.unwrap().unwrap();
      assert_eq!(msg.data, format!("{}-{}", key, i));
    }
    // drop observer and wait for worker to release
    drop(observer);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    let enqueued = metrics.reads_enqueued.get();
    let dequeued = metrics.reads_dequeued.get();
    assert_eq!(enqueued, dequeued, "dropped observer state");

    // after dropping observer, no new polling task should be rescheduled
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    assert_eq!(metrics.reads_enqueued.get(), enqueued, "unchanged enqueues");
    assert_eq!(metrics.reads_dequeued.get(), dequeued, "unchanged dequeues");

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    assert_eq!(metrics.reads_enqueued.get(), enqueued, "unchanged enqueues");
    assert_eq!(metrics.reads_dequeued.get(), dequeued, "unchanged dequeues");
  }

  fn init_streams(client: &mut Client, stream_count: usize, msgs_per_stream: usize) -> Vec<String> {
    let test_prefix: u32 = random();
    let mut keys = Vec::with_capacity(stream_count);
    for worker_idx in 0..stream_count {
      let key = format!("test:{}:{}", test_prefix, worker_idx);
      for msg_idx in 0..msgs_per_stream {
        let data = format!("{}-{}", key, msg_idx);
        let _: String = client.xadd(&key, "*", &[("data", data)]).unwrap();
      }
      keys.push(key);
    }
    keys
  }
}
