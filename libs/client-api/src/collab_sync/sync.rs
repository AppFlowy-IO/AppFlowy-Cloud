use crate::collab_sync::{
  CollabSink, CollabSinkRunner, SinkConfig, SinkState, SyncError, SyncObject,
};
use crate::platform_spawn;
use bytes::Bytes;
use collab::core::awareness::Awareness;
use collab::core::collab::MutexCollab;
use collab::core::collab_state::SyncState;
use collab::core::origin::CollabOrigin;
use futures_util::{SinkExt, StreamExt};
use realtime_entity::collab_msg::{CollabMessage, InitSync, ServerInit, UpdateSync};
use realtime_protocol::{handle_collab_message, ClientSyncProtocol, CollabSyncProtocol};
use realtime_protocol::{Message, MessageReader, SyncMessage};
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::{Arc, Weak};
use tokio::sync::watch;
use tokio_stream::wrappers::WatchStream;
use tracing::{error, event, trace, warn, Level};
use yrs::encoding::read::Cursor;
use yrs::updates::decoder::DecoderV1;
use yrs::updates::encoder::{Encoder, EncoderV1};

pub const DEFAULT_SYNC_TIMEOUT: u64 = 4;

pub struct SyncQueue<Sink, Stream> {
  object: SyncObject,
  origin: CollabOrigin,
  /// The [CollabSink] is used to send the updates to the remote. It will send the current
  /// update periodically if the timeout is reached or it will send the next update if
  /// it receive previous ack from the remote.
  sink: Arc<CollabSink<Sink, CollabMessage>>,
  /// The [SyncStream] will be spawned in a separate task It continuously receive
  /// the updates from the remote.
  #[allow(dead_code)]
  stream: SyncStream<Sink, Stream>,
  protocol: ClientSyncProtocol,
  sync_state: Arc<watch::Sender<SyncState>>,
}

impl<Sink, Stream> Drop for SyncQueue<Sink, Stream> {
  fn drop(&mut self) {
    trace!("Drop SyncQueue {}", self.object.object_id);
  }
}

impl<E, Sink, Stream> SyncQueue<Sink, Stream>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<CollabMessage, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<CollabMessage, E>> + Send + Sync + Unpin + 'static,
{
  pub fn new(
    object: SyncObject,
    origin: CollabOrigin,
    sink: Sink,
    stream: Stream,
    collab: Weak<MutexCollab>,
    config: SinkConfig,
    pause: bool,
  ) -> Self {
    let protocol = ClientSyncProtocol;
    let (notifier, notifier_rx) = watch::channel(false);
    let sync_state = Arc::new(watch::channel(SyncState::InitSyncBegin).0);
    let (sync_state_tx, sink_state_rx) = watch::channel(SinkState::Init);
    debug_assert!(origin.client_user_id().is_some());

    let sink = Arc::new(CollabSink::new(
      origin.client_user_id().unwrap_or(0),
      object.clone(),
      sink,
      notifier,
      sync_state_tx,
      config,
      pause,
    ));

    platform_spawn(CollabSinkRunner::run(Arc::downgrade(&sink), notifier_rx));
    let cloned_protocol = protocol.clone();
    let object_id = object.object_id.clone();
    let stream = SyncStream::new(
      origin.clone(),
      object_id,
      stream,
      protocol,
      collab,
      Arc::downgrade(&sink),
    );

    let weak_sync_state = Arc::downgrade(&sync_state);
    let mut sink_state_stream = WatchStream::new(sink_state_rx);
    // Subscribe the sink state stream and update the sync state in the background.
    platform_spawn(async move {
      while let Some(collab_state) = sink_state_stream.next().await {
        if let Some(sync_state) = weak_sync_state.upgrade() {
          match collab_state {
            SinkState::Syncing => {
              let _ = sync_state.send(SyncState::Syncing);
            },
            SinkState::Finished => {
              let _ = sync_state.send(SyncState::SyncFinished);
            },
            SinkState::Init => {
              let _ = sync_state.send(SyncState::InitSyncBegin);
            },
            SinkState::Pause => {},
          }
        }
      }
    });

    Self {
      object,
      origin,
      sink,
      stream,
      protocol: cloned_protocol,
      sync_state,
    }
  }

  pub fn pause(&self) {
    self.sink.pause();
  }

  pub fn resume(&self) {
    self.sink.resume();
  }

  pub fn subscribe_sync_state(&self) -> watch::Receiver<SyncState> {
    self.sync_state.subscribe()
  }

  pub fn init_sync(&self, awareness: &Awareness, _last_sync_at: i64) {
    if let Some(payload) = doc_init_state(awareness, &self.protocol) {
      self.sink.queue_init_sync(|msg_id| {
        InitSync::new(
          self.origin.clone(),
          self.object.object_id.clone(),
          self.object.collab_type.clone(),
          self.object.workspace_id.clone(),
          msg_id,
          payload,
        )
        .into()
      });
    } else {
      self.sink.notify();
    }
  }

  /// Remove all the messages in the sink queue
  pub fn clear(&self) {
    self.sink.clear();
  }
}

fn doc_init_state<P: CollabSyncProtocol>(awareness: &Awareness, protocol: &P) -> Option<Vec<u8>> {
  let payload = {
    let mut encoder = EncoderV1::new();
    protocol.start(awareness, &mut encoder).ok()?;
    encoder.to_vec()
  };
  if payload.is_empty() {
    None
  } else {
    Some(payload)
  }
}

impl<Sink, Stream> Deref for SyncQueue<Sink, Stream> {
  type Target = Arc<CollabSink<Sink, CollabMessage>>;

  fn deref(&self) -> &Self::Target {
    &self.sink
  }
}

/// Use to continuously receive updates from remote.
struct SyncStream<Sink, Stream> {
  object_id: String,
  #[allow(dead_code)]
  weak_collab: Weak<MutexCollab>,
  phantom_sink: PhantomData<Sink>,
  phantom_stream: PhantomData<Stream>,
}

impl<Sink, Stream> Drop for SyncStream<Sink, Stream> {
  fn drop(&mut self) {
    trace!("Drop SyncStream {}", self.object_id);
  }
}

impl<E, Sink, Stream> SyncStream<Sink, Stream>
where
  E: Into<anyhow::Error> + Send + Sync + 'static,
  Sink: SinkExt<CollabMessage, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<CollabMessage, E>> + Send + Sync + Unpin + 'static,
{
  pub fn new<P>(
    origin: CollabOrigin,
    object_id: String,
    stream: Stream,
    protocol: P,
    weak_collab: Weak<MutexCollab>,
    sink: Weak<CollabSink<Sink, CollabMessage>>,
  ) -> Self
  where
    P: CollabSyncProtocol + Send + Sync + 'static,
  {
    let cloned_weak_collab = weak_collab.clone();
    platform_spawn(SyncStream::<Sink, Stream>::spawn_doc_stream::<P>(
      origin,
      object_id.clone(),
      stream,
      cloned_weak_collab,
      sink,
      protocol,
    ));
    Self {
      object_id,
      weak_collab,
      phantom_sink: Default::default(),
      phantom_stream: Default::default(),
    }
  }

  // Spawn the stream that continuously reads the doc's updates from remote.
  async fn spawn_doc_stream<P>(
    origin: CollabOrigin,
    object_id: String,
    mut stream: Stream,
    weak_collab: Weak<MutexCollab>,
    weak_sink: Weak<CollabSink<Sink, CollabMessage>>,
    protocol: P,
  ) where
    P: CollabSyncProtocol + Send + Sync + 'static,
  {
    while let Some(collab_message) = stream.next().await {
      match collab_message {
        Ok(msg) => match (weak_collab.upgrade(), weak_sink.upgrade()) {
          (Some(collab), Some(sink)) => {
            let span = tracing::span!(Level::TRACE, "doc_stream", object_id = %msg.object_id());
            let _enter = span.enter();
            if let Err(error) = SyncStream::<Sink, Stream>::process_message::<P>(
              &origin, &object_id, &protocol, &collab, &sink, msg,
            )
            .await
            {
              error!("Error while processing message: {}", error);
            }
          },
          _ => {
            // The collab or sink is dropped, stop the stream.
            warn!("Stop receive doc incoming changes.");
            break;
          },
        },
        Err(e) => {
          warn!("Stream error: {},stop receive incoming changes", e.into());
          break;
        },
      }
    }
  }

  /// Continuously handle messages from the remote doc
  async fn process_message<P>(
    origin: &CollabOrigin,
    object_id: &str,
    protocol: &P,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink, CollabMessage>>,
    msg: CollabMessage,
  ) -> Result<(), SyncError>
  where
    P: CollabSyncProtocol + Send + Sync + 'static,
  {
    let should_process = match msg.msg_id() {
      // The msg_id is None if the message is [ServerBroadcast] or [ServerAwareness]
      None => true,
      Some(msg_id) => sink.ack_msg(msg.origin(), msg.object_id(), msg_id).await,
    };

    if should_process {
      if let Some(payload) = msg.payload() {
        event!(
          Level::TRACE,
          "receive collab message: {}, payload: {}",
          msg,
          payload.len()
        );
        if !payload.is_empty() {
          trace!("start process message:{:?}", msg.msg_id());
          SyncStream::<Sink, Stream>::process_payload(
            origin, payload, object_id, protocol, collab, sink,
          )
          .await?;
          trace!("end process message: {:?}", msg.msg_id());
        }
      }
    }
    Ok(())
  }

  async fn process_payload<P>(
    origin: &CollabOrigin,
    payload: &Bytes,
    object_id: &str,
    protocol: &P,
    collab: &Arc<MutexCollab>,
    sink: &Arc<CollabSink<Sink, CollabMessage>>,
  ) -> Result<(), SyncError>
  where
    P: CollabSyncProtocol + Send + Sync + 'static,
  {
    let mut decoder = DecoderV1::new(Cursor::new(payload));
    let reader = MessageReader::new(&mut decoder);
    let cloned_origin = Some(origin.clone());
    for msg in reader {
      let msg = msg?;
      trace!(" {}", msg);
      let is_sync_step_1 = matches!(msg, Message::Sync(SyncMessage::SyncStep1(_)));
      if let Some(payload) = handle_collab_message(&cloned_origin, protocol, collab, msg)? {
        if is_sync_step_1 {
          // flush
          match collab.try_lock() {
            None => warn!("Failed to acquire lock for flushing collab"),
            Some(collab_guard) => collab_guard.flush(),
          }
        }

        let object_id = object_id.to_string();
        sink.queue_msg(|msg_id| {
          if is_sync_step_1 {
            ServerInit::new(origin.clone(), object_id, payload, msg_id).into()
          } else {
            UpdateSync::new(origin.clone(), object_id, payload, msg_id).into()
          }
        });
      }
    }
    Ok(())
  }
}
