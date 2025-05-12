use super::{ObjectId, WorkspaceId};
use appflowy_proto::Rid;
use collab::preclude::Collab;
use collab_plugins::local_storage::kv::doc::CollabKVAction;
use collab_plugins::local_storage::kv::{KVStore, KVTransactionDB, PersistenceError};
use collab_plugins::local_storage::rocksdb::kv_impl::KVTransactionDBRocksdbImpl;
use rand::random;
use std::str::FromStr;
use uuid::Uuid;
use yrs::block::ClientID;
use yrs::updates::decoder::Decode;
use yrs::{ReadTxn, StateVector, Transact};

#[derive(Clone)]
pub(crate) struct Db {
  client_id: ClientID,
  uid: i64,
  workspace_id: Uuid,
  inner: KVTransactionDBRocksdbImpl,
}

impl Db {
  pub fn open(workspace_id: Uuid, uid: i64, path: &str) -> Result<Self, PersistenceError> {
    let inner = KVTransactionDBRocksdbImpl::open(path)?;
    let ops = inner.write_txn();
    let client_id = ops.client_id(&workspace_id)?;
    ops.commit_transaction()?;
    tracing::debug!("opened db for client {} - path: {}", client_id, path);
    Ok(Self {
      client_id,
      uid,
      workspace_id,
      inner,
    })
  }

  pub fn client_id(&self) -> ClientID {
    self.client_id
  }

  pub fn last_message_id(&self) -> Result<Rid, PersistenceError> {
    let ops = self.inner.write_txn();
    let message_id = ops.last_message_id(&self.workspace_id)?;
    Ok(message_id)
  }

  pub fn init_collab(&self, collab: &Collab) -> Result<bool, PersistenceError> {
    //NOTE: this shouldn't be needed, however the way how existing persistence is written,
    // it's necessary
    let collab_id: Uuid = collab.object_id().parse().unwrap();
    tracing::trace!(
      "initializing collab {}/{} in local db by {}",
      &self.workspace_id,
      collab_id,
      self.uid
    );
    let tx = collab.transact();
    let ops = self.inner.write_txn();
    match ops.create_new_doc(
      self.uid,
      &self.workspace_id.to_string(),
      &collab_id.to_string(),
      &tx,
    ) {
      Ok(_) => {
        ops.commit_transaction()?;
        Ok(true)
      },
      Err(PersistenceError::DocumentAlreadyExist) => {
        tracing::warn!("collab {} already exists in local db", collab_id);
        Ok(false)
      },
      Err(err) => Err(err),
    }
  }

  pub fn load(&self, collab: &mut Collab) -> Result<(), PersistenceError> {
    let ops = self.inner.write_txn();
    let object_id = ObjectId::from_str(collab.object_id())
      .map_err(|err| PersistenceError::InvalidData(err.to_string()))?;
    let mut txn = collab.transact_mut();
    match ops.load_doc_with_txn(
      self.uid,
      &self.workspace_id.to_string(),
      &object_id.to_string(),
      &mut txn,
    ) {
      Ok(_updates_applied) => {
        tracing::trace!("restored collab {} state: {:#?}", object_id, txn.store());
        Ok(())
      },
      Err(PersistenceError::RecordNotFound(_)) => {
        tracing::debug!("collab {} not found in local db", object_id);
        Ok(())
      },
      Err(err) => Err(err),
    }
  }

  pub fn remove_doc(&self, object_id: &Uuid) -> Result<(), PersistenceError> {
    let ops = self.inner.write_txn();
    ops.delete_doc(
      self.uid,
      &self.workspace_id.to_string(),
      &object_id.to_string(),
    )?;
    ops.commit_transaction()?;
    Ok(())
  }

  pub fn save_update(
    &self,
    object_id: &ObjectId,
    message_id: Option<Rid>,
    update_v1: &[u8],
  ) -> Result<Option<StateVector>, PersistenceError> {
    let ops = self.inner.write_txn();
    tracing::trace!(
      "persisting update for {}/{} by {}",
      self.workspace_id,
      object_id,
      self.uid
    );
    let workspace_id = self.workspace_id.to_string();
    let object_id = object_id.to_string();
    let res = ops.push_update(self.uid, &workspace_id, &object_id, update_v1);
    let mut missing = None;
    match res {
      Ok(_) => {},
      Err(PersistenceError::RecordNotFound(_)) => {
        tracing::debug!("collab {} not found in local db, initializing", object_id);
        let update = yrs::Update::decode_v1(update_v1)?;
        let doc = yrs::Doc::new();
        let mut tx = doc.transact_mut();
        tx.apply_update(update)?;
        let sv = tx.state_vector();
        if sv == StateVector::default() {
          tracing::trace!(
            "collab {} initialized in incomplete state, missing updates found",
            object_id
          );
          missing = Some(sv);
        }
        ops.create_new_doc(self.uid, &workspace_id, &object_id, &tx)?;
      },
      Err(err) => return Err(err),
    }
    if let Some(message_id) = message_id {
      ops.update_last_message_id(&self.workspace_id, message_id)?;
    }
    ops.commit_transaction()?;
    Ok(missing)
  }
}

trait CollabKVActionExt<'a>: CollabKVAction<'a>
where
  PersistenceError: From<<Self as KVStore<'a>>::Error>,
{
  fn client_id(&self, workspace_id: &Uuid) -> Result<ClientID, PersistenceError> {
    let key = keys::make_client_id_key(workspace_id);
    if let Some(existing) = self.get(&key)? {
      let slice = existing.as_ref();
      if slice.len() == 8 {
        let client_id = ClientID::from_le_bytes(slice.try_into().unwrap());
        return Ok(client_id);
      }
    }

    let client_id = random::<u64>() & ((1 << 53) - 1); // client ids are 53 bits
    tracing::trace!(
      "generated new client id {} for workspace {}",
      client_id,
      workspace_id
    );
    self.insert(key, client_id.to_le_bytes())?;
    Ok(client_id)
  }

  fn last_message_id(&self, workspace_id: &WorkspaceId) -> Result<Rid, PersistenceError> {
    let key = keys::make_last_message_id_key(workspace_id);
    match self.get(&key)? {
      None => Ok(Rid::default()),
      Some(message_id) => {
        let old_message_id = Rid::from_bytes(message_id.as_ref())
          .map_err(|e| PersistenceError::InvalidData(e.to_string()))?;
        Ok(old_message_id)
      },
    }
  }

  fn update_last_message_id(
    &self,
    workspace_id: &Uuid,
    message_id: Rid,
  ) -> Result<(), PersistenceError> {
    let old_message_id = self.last_message_id(workspace_id)?;
    let message_id = old_message_id.max(message_id);
    let key = keys::make_last_message_id_key(workspace_id);
    self.insert(key, message_id.into_bytes())?;
    tracing::trace!(
      "updated last message id for workspace {} to {}",
      workspace_id,
      message_id
    );
    Ok(())
  }
}

impl<'a, T> CollabKVActionExt<'a> for T
where
  T: CollabKVAction<'a>,
  PersistenceError: From<<T as KVStore<'a>>::Error>,
{
}

mod keys {

  // https://github.com/spacejam/sled
  // sled performs prefix encoding on long keys with similar prefixes that are grouped together in a
  // range, as well as suffix truncation to further reduce the indexing costs of long keys. Nodes
  // will skip potentially expensive length and offset pointers if keys or values are all the same
  // length (tracked separately, don't worry about making keys the same length as values), so it
  // may improve space usage slightly if you use fixed-length keys or values. This also makes it
  // easier to use structured access as well.
  //
  // DOC_SPACE
  //     DOC_SPACE_OBJECT       object_id   TERMINATOR
  //     DOC_SPACE_OBJECT_KEY     doc_id      DOC_STATE (state start)
  //     DOC_SPACE_OBJECT_KEY     doc_id      TERMINATOR_HI_WATERMARK (state end)
  //     DOC_SPACE_OBJECT_KEY     doc_id      DOC_STATE_VEC (state vector)
  //     DOC_SPACE_OBJECT_KEY     doc_id      DOC_UPDATE clock TERMINATOR (update)
  //
  // SNAPSHOT_SPACE
  //     SNAPSHOT_SPACE_OBJECT        object_id       TERMINATOR
  //     SNAPSHOT_SPACE_OBJECT_KEY    snapshot_id     SNAPSHOT_UPDATE(snapshot)
  //
  // META_SPACE (extended notation)
  //     CLIENT_ID            workspace_id  TERMINATOR
  //     LAST_MESSAGE_ID      workspace_id  TERMINATOR

  use smallvec::{smallvec, SmallVec};
  use uuid::Uuid;

  /// Prefix byte used for all metadata related keys.
  pub const META_SPACE: u8 = 3;

  /// Prefix byte used for client_id metadata for a given workspace.
  pub const CLIENT_ID: u8 = 1;

  /// Prefix byte used for last_message_id metadata for a given workspace.
  pub const LAST_MESSAGE_ID: u8 = 2;

  pub const TERMINATOR: u8 = 0;

  pub fn make_client_id_key(workspace_id: &Uuid) -> SmallVec<[u8; 19]> {
    // key: META_SPACE (1B) + CLIENT_ID (1B) + workspace_id (16B) + TERMINATOR (1B)
    let mut key = smallvec![META_SPACE, CLIENT_ID];
    key.extend_from_slice(workspace_id.as_bytes());
    key.push(TERMINATOR);
    key
  }

  pub fn make_last_message_id_key(workspace_id: &Uuid) -> SmallVec<[u8; 19]> {
    // key: META_SPACE (1B) + LAST_MESSAGE_ID (1B) + workspace_id (16B) + TERMINATOR (1B)
    let mut key = smallvec![META_SPACE, LAST_MESSAGE_ID];
    key.extend_from_slice(workspace_id.as_bytes());
    key.push(TERMINATOR);
    key
  }
}
