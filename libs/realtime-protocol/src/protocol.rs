use std::time::Duration;

use anyhow::anyhow;
use collab::core::awareness::{Awareness, AwarenessUpdate};
use collab::core::collab::{MutexCollab, TransactionMutExt};
use collab::core::origin::CollabOrigin;
use collab::core::transaction::TransactionRetry;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::{Encode, Encoder};
use yrs::{ReadTxn, StateVector, Transact, Update};

use crate::message::{CustomMessage, Error, Message, SyncMessage, SyncMeta};

// ***************************
// Client A  Client B  Server
// |          |             |
// |---(1)--Sync Step1----->|
// |          |             |
// |<--(2)--Sync Step2------|
// |<-------Sync Step1------|
// |          |             |
// |---(3)--Sync Step2----->|
// |          |             |
// **************************
// |---(1)-- Update-------->|
// |          |             |
// |          |  (2) Apply->|
// |          |             |
// |          |<-(3) Broadcast
// |          |             |
// |          |< (4) Apply  |
/// A implementation of [CollabSyncProtocol].
#[derive(Clone)]
pub struct ClientSyncProtocol;
impl CollabSyncProtocol for ClientSyncProtocol {
  fn check<E: Encoder>(&self, encoder: &mut E, last_sync_at: i64) -> Result<(), Error> {
    let meta = SyncMeta { last_sync_at };
    Message::Custom(CustomMessage::SyncCheck(meta)).encode(encoder);
    Ok(())
  }
}

pub trait CollabSyncProtocol {
  fn check<E: Encoder>(&self, _encoder: &mut E, _last_sync_at: i64) -> Result<(), Error> {
    Ok(())
  }

  fn start<E: Encoder>(&self, awareness: &Awareness, encoder: &mut E) -> Result<(), Error> {
    let (sv, update) = {
      let sv = awareness.doc().transact().state_vector();
      let update = awareness.update()?;
      (sv, update)
    };

    Message::Sync(SyncMessage::SyncStep1(sv)).encode(encoder);
    Message::Awareness(update).encode(encoder);
    Ok(())
  }

  /// Given a [StateVector] of a remote side, calculate missing
  /// updates. Returns a sync-step-2 message containing a calculated update.
  fn handle_sync_step1(
    &self,
    awareness: &Awareness,
    sv: StateVector,
  ) -> Result<Option<Vec<u8>>, Error> {
    let update = awareness
      .doc()
      .try_transact()
      .map_err(|err| Error::YrsTransaction(format!("fail to handle sync step1. error: {}", err)))?
      .encode_state_as_update_v1(&sv);
    Ok(Some(
      Message::Sync(SyncMessage::SyncStep2(update)).encode_v1(),
    ))
  }

  /// Handle reply for a sync-step-1 send from this replica previously. By default just apply
  /// an update to current `awareness` document instance.
  fn handle_sync_step2(
    &self,
    origin: &CollabOrigin,
    awareness: &mut Awareness,
    update: Update,
  ) -> Result<Option<Vec<u8>>, Error> {
    let mut retry_txn = TransactionRetry::new(awareness.doc());
    let mut txn = retry_txn
      .try_get_write_txn_with(origin.clone())
      .map_err(|err| Error::YrsTransaction(format!("sync step2 transaction acquire: {}", err)))?;
    txn
      .try_apply_update(update)
      .map_err(|err| Error::YrsApplyUpdate(format!("sync step2 apply update: {}", err)))?;
    Ok(None)
  }

  /// Handle continuous update send from the client. By default just apply an update to a current
  /// `awareness` document instance.
  fn handle_update(
    &self,
    origin: &CollabOrigin,
    awareness: &mut Awareness,
    update: Update,
  ) -> Result<Option<Vec<u8>>, Error> {
    self.handle_sync_step2(origin, awareness, update)
  }

  fn handle_auth(
    &self,
    _awareness: &Awareness,
    deny_reason: Option<String>,
  ) -> Result<Option<Vec<u8>>, Error> {
    if let Some(reason) = deny_reason {
      Err(Error::PermissionDenied { reason })
    } else {
      Ok(None)
    }
  }

  /// Reply to awareness query or just incoming [AwarenessUpdate], where current `awareness`
  /// instance is being updated with incoming data.
  fn handle_awareness_update(
    &self,
    awareness: &mut Awareness,
    update: AwarenessUpdate,
  ) -> Result<Option<Vec<u8>>, Error> {
    awareness.apply_update(update)?;
    Ok(None)
  }

  fn handle_custom_message(
    &self,
    _awareness: &mut Awareness,
    _msg: CustomMessage,
  ) -> Result<Option<Vec<u8>>, Error> {
    Ok(None)
  }
}

/// Handles incoming messages from the client/server
pub fn handle_collab_message<P: CollabSyncProtocol>(
  origin: &CollabOrigin,
  protocol: &P,
  collab: &MutexCollab,
  msg: Message,
) -> Result<Option<Vec<u8>>, Error> {
  match msg {
    Message::Sync(msg) => match msg {
      SyncMessage::SyncStep1(sv) => {
        let collab = collab
          .try_lock_for(Duration::from_millis(400))
          .ok_or(Error::Internal(anyhow!(
            "Timeout while trying to acquire lock"
          )))?;
        protocol.handle_sync_step1(collab.get_awareness(), sv)
      },
      SyncMessage::SyncStep2(update) => {
        let mut collab = collab
          .try_lock_for(Duration::from_millis(400))
          .ok_or(Error::Internal(anyhow!(
            "Timeout while trying to acquire lock"
          )))?;
        protocol.handle_sync_step2(
          origin,
          collab.get_mut_awareness(),
          Update::decode_v1(&update)?,
        )
      },
      SyncMessage::Update(update) => {
        let mut collab = collab
          .try_lock_for(Duration::from_millis(400))
          .ok_or(Error::Internal(anyhow!(
            "Timeout while trying to acquire lock"
          )))?;
        protocol.handle_update(
          origin,
          collab.get_mut_awareness(),
          Update::decode_v1(&update)?,
        )
      },
    },
    Message::Auth(reason) => {
      let collab = collab
        .try_lock_for(Duration::from_millis(400))
        .ok_or(Error::Internal(anyhow!(
          "Timeout while trying to acquire lock"
        )))?;
      protocol.handle_auth(collab.get_awareness(), reason)
    },
    Message::Awareness(update) => {
      let mut collab = collab
        .try_lock_for(Duration::from_millis(400))
        .ok_or(Error::Internal(anyhow!(
          "Timeout while trying to acquire lock"
        )))?;
      protocol.handle_awareness_update(collab.get_mut_awareness(), update)
    },
    Message::Custom(msg) => {
      let mut collab = collab
        .try_lock_for(Duration::from_millis(400))
        .ok_or(Error::Internal(anyhow!(
          "Timeout while trying to acquire lock"
        )))?;
      protocol.handle_custom_message(collab.get_mut_awareness(), msg)
    },
  }
}
