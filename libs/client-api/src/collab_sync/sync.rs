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
use collab_sync_protocol::{handle_msg, CollabSyncProtocol, DefaultSyncProtocol};
use collab_sync_protocol::{
  ClientCollabInit, ClientUpdateRequest, CollabMessage, ServerCollabInitResponse,
};
use futures_util::{SinkExt, StreamExt};
use lib0::decoding::Cursor;
use tokio::spawn;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::WatchStream;
use tracing::{error, trace, warn};
use y_sync::awareness::Awareness;
use y_sync::sync::{Message, MessageReader};
use yrs::updates::decoder::{Decode, DecoderV1};
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};

pub const DEFAULT_SYNC_TIMEOUT: u64 = 2;

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
  protocol: DefaultSyncProtocol,
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
  ) -> Self {
    let protocol = DefaultSyncProtocol;
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
          trace!("collab state change: {:?}", collab_state);
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
    match msg {
      CollabMessage::ServerInit(init_sync) => {
        if !init_sync.payload.is_empty() {
          let mut decoder = DecoderV1::from(init_sync.payload.as_ref());
          if let Ok(msg) = Message::decode(&mut decoder) {
            if let Some(resp_msg) = handle_msg(&Some(origin), protocol, collab, msg).await? {
              let payload = resp_msg.encode_v1();
              let object_id = object_id.to_string();
              sink.queue_msg(|msg_id| {
                ServerCollabInitResponse::new(origin.clone(), object_id, payload, msg_id).into()
              });
            }
          }
        }

        sink.ack_msg(object_id, init_sync.msg_id).await;
        Ok(())
      },
      _ => {
        SyncStream::<Sink, Stream>::process_payload(
          origin,
          msg.payload(),
          object_id,
          protocol,
          collab,
          sink,
        )
        .await?;
        if let Some(msg_id) = msg.msg_id() {
          sink.ack_msg(msg.object_id(), msg_id).await;
        }
        Ok(())
      },
    }
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
    if payload.is_empty() {
      return Ok(());
    }

    let mut decoder = DecoderV1::new(Cursor::new(payload));
    let reader = MessageReader::new(&mut decoder);
    for msg in reader {
      let msg = msg?;
      if let Some(resp) = handle_msg(&Some(origin), protocol, collab, msg).await? {
        let payload = resp.encode_v1();
        let object_id = object_id.to_string();
        sink.queue_msg(|msg_id| {
          ClientUpdateRequest::new(origin.clone(), object_id, msg_id, payload).into()
        });
      }
    }
    Ok(())
  }
}
