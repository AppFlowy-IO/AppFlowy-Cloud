use collab::core::awareness::{Awareness, AwarenessUpdate};
use collab::core::collab::{TransactionExt, TransactionMutExt};
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;

use yrs::updates::decoder::Decode;
use yrs::updates::encoder::{Encode, Encoder};
use yrs::{ReadTxn, StateVector, Transact, Update};

use crate::message::{CustomMessage, Message, RTProtocolError, SyncMessage, SyncMeta};

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
  fn check<E: Encoder>(&self, encoder: &mut E, last_sync_at: i64) -> Result<(), RTProtocolError> {
    let meta = SyncMeta { last_sync_at };
    Message::Custom(CustomMessage::SyncCheck(meta)).encode(encoder);
    Ok(())
  }

  /// Handle reply for a sync-step-1 send from this replica previously. By default just apply
  /// an update to current `awareness` document instance.
  fn handle_sync_step2(
    &self,
    origin: &CollabOrigin,
    awareness: &mut Awareness,
    update: Update,
  ) -> Result<(), RTProtocolError> {
    let mut txn = awareness
      .doc()
      .try_transact_mut_with(origin.clone())
      .map_err(|err| {
        RTProtocolError::YrsTransaction(format!("sync step2 transaction acquire: {}", err))
      })?;
    txn.try_apply_update(update).map_err(|err| {
      RTProtocolError::YrsApplyUpdate(format!("sync step2 apply update: {} ", err))
    })?;

    // If the client can't apply broadcast from server, which means the client is missing some
    // updates.
    match txn.store().pending_update() {
      Some(update) => {
        if cfg!(feature = "verbose_log") {
          tracing::trace!(
            "Did find pending update, missing: {}",
            update.missing.is_empty()
          );
        }
        // let state_vector_v1 = update.missing.encode_v1();
        Err(RTProtocolError::MissUpdates {
          state_vector_v1: None,
          reason: "client miss updates".to_string(),
        })
      },
      None => Ok(()),
    }
  }
}

pub trait CollabSyncProtocol {
  fn check<E: Encoder>(&self, _encoder: &mut E, _last_sync_at: i64) -> Result<(), RTProtocolError> {
    Ok(())
  }

  fn start<E: Encoder>(
    &self,
    awareness: &Awareness,
    encoder: &mut E,
    _sync_before: bool,
  ) -> Result<(), RTProtocolError> {
    let (state_vector, awareness_update) = {
      let state_vector = awareness
        .doc()
        .try_transact()
        .map_err(|e| RTProtocolError::YrsTransaction(e.to_string()))?
        .state_vector();
      let awareness_update = awareness.update()?;
      (state_vector, awareness_update)
    };

    // 1. encode doc state vector
    Message::Sync(SyncMessage::SyncStep1(state_vector)).encode(encoder);

    // // 2. if the sync_before is false, which means the doc is not synced before, then we need to
    // // send the full update to the server.
    // if !sync_before {
    //   if let Ok(txn) = awareness.doc().try_transact() {
    //     let update = txn.encode_state_as_update_v1(&StateVector::default());
    //     Message::Sync(SyncMessage::SyncStep2(update)).encode(encoder);
    //   }
    // }

    // 3. encode awareness update
    Message::Awareness(awareness_update).encode(encoder);
    Ok(())
  }

  /// Given a [StateVector] of a remote side, calculate missing
  /// updates. Returns a sync-step-2 message containing a calculated update.
  fn handle_sync_step1(
    &self,
    awareness: &Awareness,
    sv: StateVector,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    let txn = awareness.doc().try_transact().map_err(|err| {
      RTProtocolError::YrsTransaction(format!("fail to handle sync step1. error: {}", err))
    })?;
    let update = txn.try_encode_state_as_update_v1(&sv).map_err(|err| {
      RTProtocolError::YrsEncodeState(format!(
        "fail to encode state as update. error: {}\ninit state vector: {:?}\ndocument state: {:#?}",
        err,
        sv,
        txn.store()
      ))
    })?;
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
  ) -> Result<(), RTProtocolError>;

  /// Handle continuous update send from the client. By default just apply an update to a current
  /// `awareness` document instance.
  fn handle_update(
    &self,
    origin: &CollabOrigin,
    awareness: &mut Awareness,
    update: Update,
  ) -> Result<(), RTProtocolError> {
    self.handle_sync_step2(origin, awareness, update)
  }

  fn handle_auth(
    &self,
    _awareness: &Awareness,
    deny_reason: Option<String>,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    if let Some(reason) = deny_reason {
      Err(RTProtocolError::PermissionDenied { reason })
    } else {
      Ok(None)
    }
  }

  /// Reply to awareness query or just incoming [AwarenessUpdate], where current `awareness`
  /// instance is being updated with incoming data.
  fn handle_awareness_update(
    &self,
    _message_origin: &CollabOrigin,
    awareness: &mut Awareness,
    update: AwarenessUpdate,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    awareness.apply_update(update)?;
    Ok(None)
  }

  fn handle_custom_message(
    &self,
    _awareness: &mut Awareness,
    _msg: CustomMessage,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    Ok(None)
  }
}

/// Handles incoming messages from the client/server
pub fn handle_message_follow_protocol<P: CollabSyncProtocol>(
  message_origin: &CollabOrigin,
  protocol: &P,
  collab: &mut Collab,
  msg: Message,
) -> Result<Option<Vec<u8>>, RTProtocolError> {
  match msg {
    Message::Sync(msg) => match msg {
      SyncMessage::SyncStep1(sv) => {
        // calculate missing updates base on the input state vector
        let update = protocol.handle_sync_step1(collab.get_awareness(), sv)?;
        Ok(update)
      },
      SyncMessage::SyncStep2(update) => {
        protocol.handle_sync_step2(
          message_origin,
          collab.get_mut_awareness(),
          Update::decode_v1(&update)?,
        )?;
        Ok(None)
      },
      SyncMessage::Update(update) => {
        protocol.handle_update(
          message_origin,
          collab.get_mut_awareness(),
          Update::decode_v1(&update)?,
        )?;
        Ok(None)
      },
    },
    Message::Auth(reason) => protocol.handle_auth(collab.get_awareness(), reason),
    Message::Awareness(update) => {
      protocol.handle_awareness_update(message_origin, collab.get_mut_awareness(), update)
    },
    Message::Custom(msg) => protocol.handle_custom_message(collab.get_mut_awareness(), msg),
  }
}
