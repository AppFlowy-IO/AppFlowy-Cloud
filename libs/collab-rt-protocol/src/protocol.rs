use std::borrow::BorrowMut;
use std::sync::{Arc, Weak};

use async_trait::async_trait;
use collab::core::awareness::{Awareness, AwarenessUpdate};
use collab::core::collab::TransactionMutExt;
use collab::core::origin::CollabOrigin;
use collab::lock::RwLock;
use collab::preclude::Collab;
use tokio::task::spawn_blocking;
use yrs::updates::decoder::Decode;
use yrs::updates::encoder::{Encode, Encoder};
use yrs::{ReadTxn, StateVector, Transact, Update};

use crate::message::{CustomMessage, Message, RTProtocolError, SyncMessage, SyncMeta};

/// A implementation of [CollabSyncProtocol].
#[derive(Clone)]
pub struct ClientSyncProtocol;

#[async_trait]
impl CollabSyncProtocol for ClientSyncProtocol {
  fn check<E: Encoder>(&self, encoder: &mut E, last_sync_at: i64) -> Result<(), RTProtocolError> {
    let meta = SyncMeta { last_sync_at };
    Message::Custom(CustomMessage::SyncCheck(meta)).encode(encoder);
    Ok(())
  }

  /// Handle reply for a sync-step-1 send from this replica previously. By default just apply
  /// an update to current `awareness` document instance.
  async fn handle_sync_step2(
    &self,
    origin: &CollabOrigin,
    collab: &CollabRef,
    update: Vec<u8>,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    let update = decode_update(update).await?;
    let mut lock = collab.write().await;
    let collab = (*lock).borrow_mut();
    let mut txn = collab
      .get_awareness()
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

        // when client handle sync step 2 and found missing updates, just return MissUpdates Error.
        // the state vector should be none that will trigger a client init sync
        Err(RTProtocolError::MissUpdates {
          state_vector_v1: None,
          reason: "client miss updates".to_string(),
        })
      },
      None => Ok(None),
    }
  }
}

pub type CollabRef = Arc<RwLock<dyn BorrowMut<Collab> + Send + Sync + 'static>>;

pub type WeakCollabRef = Weak<RwLock<dyn BorrowMut<Collab> + Send + Sync + 'static>>;

#[async_trait]
pub trait CollabSyncProtocol {
  /// Handles incoming messages from the client/server
  async fn handle_message(
    &self,
    message_origin: &CollabOrigin,
    collab: &CollabRef,
    msg: Message,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    match msg {
      Message::Sync(msg) => match msg {
        SyncMessage::SyncStep1(sv) => self.handle_sync_step1(collab, sv).await,
        SyncMessage::SyncStep2(update) => {
          self.handle_sync_step2(message_origin, collab, update).await
        },
        SyncMessage::Update(update) => self.handle_update(message_origin, collab, update).await,
      },
      Message::Auth(reason) => self.handle_auth(collab, reason).await,
      //FIXME: where is the QueryAwareness protocol?
      Message::Awareness(update) => {
        let update = AwarenessUpdate::decode_v1(&update)?;
        self
          .handle_awareness_update(message_origin, collab, update)
          .await
      },
      Message::Custom(msg) => self.handle_custom_message(collab, msg).await,
    }
  }

  fn check<E: Encoder>(&self, _encoder: &mut E, _last_sync_at: i64) -> Result<(), RTProtocolError> {
    Ok(())
  }

  fn start<E: Encoder>(
    &self,
    awareness: &Awareness,
    encoder: &mut E,
  ) -> Result<(), RTProtocolError> {
    let (state_vector, awareness_update) = {
      let state_vector = awareness
        .doc()
        .try_transact()
        .map_err(|e| RTProtocolError::YrsTransaction(e.to_string()))?
        .state_vector();
      let awareness_update = awareness.update()?;
      (state_vector, awareness_update.encode_v1())
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
  async fn handle_sync_step1(
    &self,
    collab: &CollabRef,
    sv: StateVector,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    // calculate missing updates base on the input state vector
    let update = {
      let lock = collab.read().await;
      let collab = lock.borrow();
      let txn = collab.get_awareness().doc().try_transact().map_err(|err| {
        RTProtocolError::YrsTransaction(format!("fail to handle sync step1. error: {}", err))
      })?;
      txn.encode_diff_v1(&sv)
    };
    Ok(Some(
      Message::Sync(SyncMessage::SyncStep2(update)).encode_v1(),
    ))
  }

  /// Handle reply for a sync-step-1 send from this replica previously. By default just apply
  /// an update to current `awareness` document instance.
  async fn handle_sync_step2(
    &self,
    origin: &CollabOrigin,
    collab: &CollabRef,
    update: Vec<u8>,
  ) -> Result<Option<Vec<u8>>, RTProtocolError>;

  /// Handle continuous update send from the client. By default just apply an update to a current
  /// `awareness` document instance.
  async fn handle_update(
    &self,
    origin: &CollabOrigin,
    collab: &CollabRef,
    update: Vec<u8>,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    self.handle_sync_step2(origin, collab, update).await
  }

  async fn handle_auth(
    &self,
    _collab: &CollabRef,
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
  async fn handle_awareness_update(
    &self,
    _message_origin: &CollabOrigin,
    collab: &CollabRef,
    update: AwarenessUpdate,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    let mut lock = collab.write().await;
    let collab = (*lock).borrow_mut();
    collab.get_awareness().apply_update(update)?;
    Ok(None)
  }

  async fn handle_custom_message(
    &self,
    _collab: &CollabRef,
    _msg: CustomMessage,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    Ok(None)
  }
}

pub const LARGE_UPDATE_THRESHOLD: usize = 1024 * 1024; // 1MB

#[inline]
pub async fn decode_update(update: Vec<u8>) -> Result<Update, yrs::encoding::read::Error> {
  let update = if update.len() > LARGE_UPDATE_THRESHOLD {
    spawn_blocking(move || Update::decode_v1(&update))
      .await
      .map_err(|err| yrs::encoding::read::Error::Custom(err.to_string()))?
  } else {
    Update::decode_v1(&update)
  }?;
  Ok(update)
}
