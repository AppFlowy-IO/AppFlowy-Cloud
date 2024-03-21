use collab::core::awareness::Awareness;
use collab_rt_protocol::CollabSyncProtocol;
use collab_rt_protocol::{CustomMessage, Error, Message, SyncMessage};
use yrs::updates::encoder::{Encode, Encoder, EncoderV1};
use yrs::{ReadTxn, StateVector, Transact};

#[derive(Clone)]
pub struct ServerSyncProtocol;
impl CollabSyncProtocol for ServerSyncProtocol {
  fn handle_sync_step1(
    &self,
    awareness: &Awareness,
    sv: StateVector,
  ) -> Result<Option<Vec<u8>>, Error> {
    let txn = awareness.doc().transact();
    let client_step2_update = txn.encode_state_as_update_v1(&sv);

    // Retrieve the latest document state from the client after they return online from offline editing.
    let server_step1_update = txn.state_vector();

    let mut encoder = EncoderV1::new();
    Message::Sync(SyncMessage::SyncStep2(client_step2_update)).encode(&mut encoder);
    Message::Sync(SyncMessage::SyncStep1(server_step1_update)).encode(&mut encoder);
    Ok(Some(encoder.to_vec()))
  }

  fn handle_custom_message(
    &self,
    _awareness: &mut Awareness,
    _msg: CustomMessage,
  ) -> Result<Option<Vec<u8>>, Error> {
    Ok(None)
  }
}
