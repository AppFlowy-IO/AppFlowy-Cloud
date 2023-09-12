use crate::collab::{CollabBroadcast, Subscription};
use crate::error::RealtimeError;
use bytes::{Bytes, BytesMut};
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_sync_protocol::CollabMessage;
use parking_lot::RwLock;
use std::collections::HashMap;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::task::spawn_blocking;
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite, LengthDelimitedCodec};

/// A group used to manage a single [Collab] object
pub struct CollabGroup {
  pub collab: MutexCollab,

  /// A broadcast used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
  /// to subscribes.
  pub broadcast: CollabBroadcast,

  /// A list of subscribers to this group. Each subscriber will receive updates from the
  /// broadcast.
  pub subscribers: RwLock<HashMap<CollabOrigin, Subscription>>,
}

impl CollabGroup {
  /// Mutate the [Collab] by the given closure
  pub fn get_mut_collab<F>(&self, f: F)
  where
    F: FnOnce(&Collab),
  {
    let collab = self.collab.lock();
    f(&collab);
  }

  pub fn is_empty(&self) -> bool {
    self.subscribers.read().is_empty()
  }

  /// Flush the [Collab] to the storage.
  /// When there is no subscriber, perform the flush in a blocking task.
  pub fn flush_collab(&self) {
    let collab = self.collab.clone();
    spawn_blocking(move || {
      collab.lock().flush();
    });
  }
}

#[derive(Debug, Default)]
pub struct CollabMsgCodec(LengthDelimitedCodec);

impl Encoder<CollabMessage> for CollabMsgCodec {
  type Error = RealtimeError;

  fn encode(&mut self, item: CollabMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
    let bytes = item.to_vec();
    self.0.encode(Bytes::from(bytes), dst)?;
    Ok(())
  }
}

impl Decoder for CollabMsgCodec {
  type Item = CollabMessage;
  type Error = RealtimeError;

  fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
    if let Some(bytes) = self.0.decode(src)? {
      let bytes = bytes.freeze().to_vec();
      let msg = CollabMessage::from_vec(&bytes).ok();
      Ok(msg)
    } else {
      Ok(None)
    }
  }
}

pub type CollabStream = FramedRead<OwnedReadHalf, CollabMsgCodec>;
pub type CollabSink = FramedWrite<OwnedWriteHalf, CollabMsgCodec>;
