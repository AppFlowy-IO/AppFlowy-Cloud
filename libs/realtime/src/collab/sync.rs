use crate::collab::{CollabBroadcast, Subscription};
use crate::error::CollabSyncError;
use bytes::{Bytes, BytesMut};
use collab::core::collab::MutexCollab;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_sync_protocol::CollabMessage;
use std::collections::HashMap;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio_util::codec::{Decoder, Encoder, FramedRead, FramedWrite, LengthDelimitedCodec};

/// A group used to manage a single document
pub struct CollabGroup {
  /// [Collab] wrapped in a mutex
  pub collab: MutexCollab,

  /// A broadcast used to propagate updates produced by yrs [yrs::Doc] and [Awareness]
  /// to subscribes.
  pub broadcast: CollabBroadcast,

  /// A list of subscribers to this group. Each subscriber will receive updates from the
  /// broadcast.
  pub subscribers: HashMap<CollabOrigin, Subscription>,
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
}

#[derive(Debug, Default)]
pub struct CollabMsgCodec(LengthDelimitedCodec);

impl Encoder<CollabMessage> for CollabMsgCodec {
  type Error = CollabSyncError;

  fn encode(&mut self, item: CollabMessage, dst: &mut BytesMut) -> Result<(), Self::Error> {
    let bytes = item.to_vec();
    self.0.encode(Bytes::from(bytes), dst)?;
    Ok(())
  }
}

impl Decoder for CollabMsgCodec {
  type Item = CollabMessage;
  type Error = CollabSyncError;

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
