use bytes::Bytes;
use std::marker::PhantomData;
use std::ops::Deref;
use std::sync::{Arc, Weak};

use crate::collab_sync::{
  CollabSink, CollabSinkRunner, DefaultMsgIdCounter, SinkConfig, SinkState, SyncError, SyncObject,
};
use collab::core::collab::MutexCollab;
use collab::core::collab_state::SyncState;
use collab::core::origin::CollabOrigin;
use collab::sync_protocol::awareness::Awareness;
use collab::sync_protocol::message::{Message, MessageReader, SyncMessage};
use collab::sync_protocol::{handle_msg, ClientSyncProtocol, CollabSyncProtocol};
use futures_util::{SinkExt, StreamExt};
use lib0::decoding::Cursor;
use realtime_entity::collab_msg::{ClientCollabInit, CollabMessage, ServerCollabInit, UpdateSync};
use tokio::spawn;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::WatchStream;
use tracing::{error, trace, warn};
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

impl<E, Sink, Stream> SyncQueue<Sink, Stream>
where
  E: std::error::Error + Send + Sync + 'static,
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
    let sync_state = Arc::new(watch::channel(SyncState::SyncInitStart).0);
    let (sync_state_tx, sink_state_rx) = watch::channel(SinkState::Init);
    debug_assert!(origin.client_user_id().is_some());

    let sink = Arc::new(CollabSink::new(
      origin.client_user_id().unwrap_or(0),
      sink,
      notifier,
      sync_state_tx,
      DefaultMsgIdCounter::new(),
      config,
      pause,
    ));

    spawn(CollabSinkRunner::run(Arc::downgrade(&sink), notifier_rx));
    let cloned_protocol = protocol.clone();
    let object_id = object.object_id.clone();
    let stream = SyncStream::new(
      origin.clone(),
      object_id,
      stream,
      protocol,
      collab,
      sink.clone(),
    );

    let weak_sync_state = Arc::downgrade(&sync_state);
    let mut sink_state_stream = WatchStream::new(sink_state_rx);
    // Subscribe the sink state stream and update the sync state in the background.
    spawn(async move {
      while let Some(collab_state) = sink_state_stream.next().await {
        if let Some(sync_state) = weak_sync_state.upgrade() {
          match collab_state {
            SinkState::Syncing => {
              let _ = sync_state.send(SyncState::SyncUpdate);
            },
            SinkState::Finished => {
              let _ = sync_state.send(SyncState::SyncFinished);
            },
            SinkState::Init => {
              let _ = sync_state.send(SyncState::SyncInitStart);
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

  pub fn init_sync(&self, awareness: &Awareness) {
    if let Some(payload) = doc_init_state(awareness, &self.protocol) {
      self.sink.queue_init_sync(|msg_id| {
        ClientCollabInit::new(
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
  #[allow(dead_code)]
  weak_collab: Weak<MutexCollab>,
  #[allow(dead_code)]
  runner: JoinHandle<Result<(), SyncError>>,
  phantom_sink: PhantomData<Sink>,
  phantom_stream: PhantomData<Stream>,
}

impl<E, Sink, Stream> SyncStream<Sink, Stream>
where
  E: std::error::Error + Send + Sync + 'static,
  Sink: SinkExt<CollabMessage, Error = E> + Send + Sync + Unpin + 'static,
  Stream: StreamExt<Item = Result<CollabMessage, E>> + Send + Sync + Unpin + 'static,
{
  pub fn new<P>(
    origin: CollabOrigin,
    object_id: String,
    stream: Stream,
    protocol: P,
    weak_collab: Weak<MutexCollab>,
    sink: Arc<CollabSink<Sink, CollabMessage>>,
  ) -> Self
  where
    P: CollabSyncProtocol + Send + Sync + 'static,
  {
    let cloned_weak_collab = weak_collab.clone();
    let weak_sink = Arc::downgrade(&sink);
    let runner = spawn(SyncStream::<Sink, Stream>::spawn_doc_stream::<P>(
      origin,
      object_id,
      stream,
      cloned_weak_collab,
      weak_sink,
      protocol,
    ));
    Self {
      weak_collab,
      runner,
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
  ) -> Result<(), SyncError>
  where
    P: CollabSyncProtocol + Send + Sync + 'static,
  {
    while let Some(input) = stream.next().await {
      match input {
        Ok(msg) => match (weak_collab.upgrade(), weak_sink.upgrade()) {
          (Some(awareness), Some(sink)) => {
            SyncStream::<Sink, Stream>::process_message::<P>(
              &origin, &object_id, &protocol, &awareness, &sink, msg,
            )
            .await?
          },
          _ => {
            warn!("ClientSync is dropped. Stopping receive incoming changes.");
            return Ok(());
          },
        },
        Err(e) => {
          error!("Spawn doc stream failed: {}", e);
          // If the client has disconnected, the stream will return an error, So stop receiving
          // messages if the client has disconnected.
          return Err(SyncError::Internal(Box::new(e)));
        },
      }
    }
    Ok(())
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
    if match msg.msg_id() {
      // The msg_id is None if the message is [ServerBroadcast] or [ServerAwareness]
      None => true,
      Some(msg_id) => sink.ack_msg(msg.origin(), msg.object_id(), msg_id).await,
    } && !msg.payload().is_empty()
    {
      trace!("start process message: {:?}", msg.msg_id());
      SyncStream::<Sink, Stream>::process_payload(
        origin,
        msg.payload(),
        object_id,
        protocol,
        collab,
        sink,
      )
      .await?;
      trace!("end process message: {:?}", msg.msg_id());
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
    for msg in reader {
      let msg = msg?;
      trace!(" {}", msg);
      let is_sync_step_1 = matches!(msg, Message::Sync(SyncMessage::SyncStep1(_)));
      if let Some(payload) = handle_msg(&Some(origin), protocol, collab, msg).await? {
        if is_sync_step_1 {
          // flush
          collab.lock().flush()
        }

        let object_id = object_id.to_string();
        sink.queue_msg(|msg_id| {
          if is_sync_step_1 {
            ServerCollabInit::new(origin.clone(), object_id, payload, msg_id).into()
          } else {
            UpdateSync::new(origin.clone(), object_id, payload, msg_id).into()
          }
        });
      }
    }
    Ok(())
  }
}
