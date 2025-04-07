use super::{ObjectId, WorkspaceId};
use appflowy_proto::Rid;
use collab::core::origin::{CollabClient, CollabOrigin};
use collab::preclude::Collab;
use collab_plugins::local_storage::kv::doc::CollabKVAction;
use collab_plugins::local_storage::kv::{KVStore, PersistenceError};
use collab_plugins::local_storage::rocksdb::kv_impl::RocksdbKVStoreImpl;
use rand::random;
use rocksdb::TransactionDB;
use std::str::FromStr;
use uuid::Uuid;
use yrs::block::ClientID;
use yrs::{Doc, Options, StateVector, Transact, Update};

pub(crate) struct Db {
  client_id: ClientID,
  uid: i64,
  device_id: String,
  workspace_id: Uuid,
  inner: TransactionDB,
}

impl Db {
  pub fn open(
    workspace_id: Uuid,
    uid: i64,
    device_id: String,
    path: &str,
  ) -> Result<Self, PersistenceError> {
    let inner = TransactionDB::open_default(path)?;
    let tx = inner.transaction();
    let ops = RocksdbKVStoreImpl::new(tx);
    let client_id = ops.client_id(&workspace_id)?;
    ops.commit_transaction()?;
    tracing::debug!("opened db for client {} - path: {}", client_id, path);
    Ok(Self {
      client_id,
      uid,
      device_id,
      workspace_id,
      inner,
    })
  }

  pub fn client_id(&self) -> ClientID {
    self.client_id
  }

  pub fn last_message_id(&self) -> Result<Rid, PersistenceError> {
    let ops = RocksdbKVStoreImpl::new(self.inner.transaction());
    let message_id = ops.last_message_id(&self.workspace_id)?;
    Ok(message_id)
  }

  pub fn load(&self, collab: &mut Collab) -> Result<(), PersistenceError> {
    let ops = RocksdbKVStoreImpl::new(self.inner.transaction());
    let object_id = ObjectId::from_str(collab.object_id())
      .map_err(|err| PersistenceError::InvalidData(err.to_string()))?;
    let mut txn = collab.transact_mut();
    ops.load_doc_with_txn(self.uid, &self.workspace_id, &object_id, &mut txn)?;
    drop(txn);
    Ok(())
  }

  pub fn remove_doc(&self, object_id: &Uuid) -> Result<(), PersistenceError> {
    let tx = self.inner.transaction();
    let ops = RocksdbKVStoreImpl::new(tx);
    ops.delete_doc(self.uid, &self.workspace_id, object_id)?;
    ops.commit_transaction()?;
    Ok(())
  }

  pub fn save_update(
    &self,
    object_id: &ObjectId,
    message_id: Option<Rid>,
    update_v1: Vec<u8>,
  ) -> Result<(), PersistenceError> {
    let tx = self.inner.transaction();
    let ops = RocksdbKVStoreImpl::new(tx);
    if let Some(message_id) = message_id {
      ops.update_last_message_id(&self.workspace_id, message_id)?;
    }
    ops.push_update(self.uid, &self.workspace_id, object_id, &update_v1)?;
    ops.commit_transaction()?;
    tracing::trace!("persisted update for {}", object_id);
    Ok(())
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
    self.insert(key, &client_id.to_le_bytes())?;
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
    self.insert(key, &message_id.into_bytes())?;
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
