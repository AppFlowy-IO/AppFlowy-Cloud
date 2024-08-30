use std::sync::Arc;

use async_trait::async_trait;
use collab::core::collab::{TransactionExt, TransactionMutExt};
use collab::core::origin::CollabOrigin;
use yrs::{ReadTxn, StateVector, Transact};
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};

use collab_rt_protocol::{
  CollabRef, CustomMessage, decode_update, Message, RTProtocolError, SyncMessage,
};
use collab_rt_protocol::CollabSyncProtocol;

use crate::CollabRealtimeMetrics;

#[derive(Clone)]
pub struct ServerSyncProtocol(Arc<CollabRealtimeMetrics>);

impl ServerSyncProtocol {
  pub fn new(metrics: &Arc<CollabRealtimeMetrics>) -> Self {
    Self(metrics.clone())
  }
}

#[async_trait]
impl CollabSyncProtocol for ServerSyncProtocol {
  async fn handle_sync_step1(
    &self,
    collab: &CollabRef,
    sv: StateVector,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    let (doc_state, state_vector) = {
      let lock = collab.read().await;
      let collab = (*lock).borrow();

      let txn = collab.get_awareness().doc().try_transact().map_err(|err| {
        RTProtocolError::YrsTransaction(format!("fail to handle sync step1. error: {}", err))
      })?;

      let doc_state = txn.try_encode_state_as_update_v1(&sv).map_err(|err| {
        RTProtocolError::YrsEncodeState(format!(
        "fail to encode state as update. error: {}\ninit state vector: {:?}\ndocument state: {:#?}",
        err,
        sv,
        txn.store()
      ))
      })?;
      (doc_state, txn.state_vector())
    };

    // Retrieve the latest document state from the client after they return online from offline editing.
    let mut encoder = EncoderV1::new();
    Message::Sync(SyncMessage::SyncStep2(doc_state)).encode(&mut encoder);

    //FIXME: this should never happen as response to sync step 1 from the client, but rather be
    //  send when a connection is established
    Message::Sync(SyncMessage::SyncStep1(state_vector)).encode(&mut encoder);
    Ok(Some(encoder.to_vec()))
  }

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
      RTProtocolError::YrsApplyUpdate(format!(
        "sync step2 apply update: {}\ndocument state: {:#?}",
        err,
        txn.store()
      ))
    })?;

    // If server can't apply updates sent by client, which means the server is missing some updates
    // from the client or the client is missing some updates from the server.
    // If the client can't apply broadcast from server, which means the client is missing some
    // updates.
    match txn.store().pending_update() {
      Some(_update) => {
        // let state_vector_v1 = update.missing.encode_v1();
        // for the moment, we don't need to send missing updates to the client. passing None
        // instead, which will trigger a sync step 0 on client
        let state_vector_v1 = txn.state_vector().encode_v1();
        Err(RTProtocolError::MissUpdates {
          state_vector_v1: Some(state_vector_v1),
          reason: "server miss updates".to_string(),
        })
      },
      None => Ok(None),
    }
  }

  async fn handle_custom_message(
    &self,
    _collab: &CollabRef,
    _msg: CustomMessage,
  ) -> Result<Option<Vec<u8>>, RTProtocolError> {
    Ok(None)
  }
}
