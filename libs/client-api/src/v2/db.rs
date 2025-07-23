use super::{ObjectId, WorkspaceId};
use crate::v2::actor::ActionSource;
use crate::{sync_debug, sync_error, sync_trace};
use anyhow::anyhow;
use appflowy_proto::Rid;
use client_api_entity::CollabType;
use collab::core::collab::CollabOptions;
use collab::core::origin::CollabOrigin;
use collab::core::transaction::DocTransactionExtension;
use collab::preclude::Collab;
use collab_plugins::local_storage::kv::doc::CollabKVAction;
use collab_plugins::local_storage::kv::{KVStore, KVTransactionDB, PersistenceError};
use collab_plugins::local_storage::rocksdb::kv_impl::KVTransactionDBRocksdbImpl;
use rand::random;
use std::str::FromStr;
use std::sync::{Arc, Weak};
use tracing::instrument;
use uuid::Uuid;
use yrs::block::ClientID;
use yrs::updates::decoder::Decode;
use yrs::{ReadTxn, StateVector, Transact, Update};

#[derive(Clone)]
pub(crate) struct Db {
  client_id: ClientID,
  uid: i64,
  workspace_id: Uuid,
  inner: DbHolder,
}

impl Db {
  pub fn open(workspace_id: Uuid, uid: i64, path: &str) -> Result<Self, PersistenceError> {
    let inner = KVTransactionDBRocksdbImpl::open(path)?;
    let this = Self::open_with_rocksdb(workspace_id, uid, inner)?;
    Ok(this)
  }

  pub fn open_with_rocksdb<T: Into<DbHolder>>(
    workspace_id: Uuid,
    uid: i64,
    db: T,
  ) -> Result<Self, PersistenceError> {
    let db = db.into();
    let instance = db.get()?;
    let ops = instance.write_txn();
    let client_id = ops.client_id(&workspace_id)?;
    ops.commit_transaction()?;

    sync_debug!("db client_id {}", client_id);
    Ok(Self {
      client_id,
      uid,
      workspace_id,
      inner: db,
    })
  }

  pub fn client_id(&self) -> ClientID {
    self.client_id
  }

  pub fn last_message_id(&self) -> Result<Rid, PersistenceError> {
    let message_id = self
      .inner
      .get()?
      .write_txn()
      .last_message_id(&self.workspace_id)?;
    Ok(message_id)
  }

  pub fn init_collab(
    &self,
    object_id: &ObjectId,
    collab: &Collab,
    collab_type: &CollabType,
  ) -> Result<bool, PersistenceError> {
    sync_trace!(
      "initializing collab {}/{}/{} in local db by {}",
      &self.workspace_id,
      object_id,
      collab_type,
      self.uid
    );
    let tx = collab.transact();
    let instance = self.inner.get()?;
    let ops = instance.write_txn();

    let workspace_id = self.workspace_id.to_string();
    let object_id = object_id.to_string();

    if !ops.is_exist(self.uid, &workspace_id, &object_id) {
      match ops.create_new_doc(self.uid, &workspace_id, &object_id, &tx) {
        Ok(_) => {
          ops.commit_transaction()?;
          sync_trace!(
            "Save collab {}/{}/{} to local db. store: {:#?}",
            &self.workspace_id,
            object_id,
            collab_type,
            tx.store()
          );
          Ok(true)
        },
        Err(PersistenceError::DocumentAlreadyExist) => Ok(false),
        Err(err) => Err(err),
      }
    } else {
      Ok(false)
    }
  }

  #[allow(dead_code)]
  pub fn get_state_vector(&self, object_id: &ObjectId) -> Result<StateVector, PersistenceError> {
    let instance = self.inner.get()?;
    let ops = instance.read_txn();
    let object_id = object_id.to_string();
    let options = CollabOptions::new(object_id.to_string(), self.client_id);
    let mut collab = Collab::new_with_options(CollabOrigin::Empty, options)?;
    let mut txn = collab.transact_mut();
    ops.load_doc_with_txn(
      self.uid,
      &self.workspace_id.to_string(),
      &object_id.to_string(),
      &mut txn,
    )?;

    Ok(txn.state_vector())
  }

  pub fn batch_get_state_vector(
    &self,
    object_ids: &[&ObjectId],
  ) -> Result<Vec<(ObjectId, StateVector)>, PersistenceError> {
    let instance = self.inner.get()?;
    let ops = instance.read_txn();
    let mut result = Vec::new();

    for object_id in object_ids {
      let get_state_vector = || -> Result<StateVector, PersistenceError> {
        let object_id_str = object_id.to_string();
        let options = CollabOptions::new(object_id_str.to_string(), self.client_id);
        let mut collab = Collab::new_with_options(CollabOrigin::Empty, options)?;
        let mut txn = collab.transact_mut();

        ops.load_doc_with_txn(
          self.uid,
          &self.workspace_id.to_string(),
          &object_id_str,
          &mut txn,
        )?;

        Ok(txn.state_vector())
      };

      match get_state_vector() {
        Ok(state_vector) => result.push((**object_id, state_vector)),
        Err(_) => continue,
      }
    }

    Ok(result)
  }

  /// Loads a document from the local database into the provided Collab instance.
  ///
  /// This function retrieves updates from the local database and applies it to the given
  /// object. When the flush parameter is enabled, it will remove all individual updates and
  /// store only the final document state and state vector,
  ///
  /// # Parameters
  /// * `collab` - The mutable Collab instance to load the document data into
  /// * `flush` - When true, consolidates storage by replacing all individual updates with
  ///   the final document state
  pub fn load(&self, collab: &mut Collab, flush: bool) -> Result<(), PersistenceError> {
    let instance = self.inner.get()?;
    let ops = instance.write_txn();
    let object_id = ObjectId::from_str(collab.object_id())
      .map_err(|err| PersistenceError::InvalidData(err.to_string()))?;
    {
      let mut txn = collab.transact_mut();
      match ops.load_doc_with_txn(
        self.uid,
        &self.workspace_id.to_string(),
        &object_id.to_string(),
        &mut txn,
      ) {
        Ok(updates_applied) => {
          sync_trace!(
            "restored collab {}, apply updates:{}, state: {:#?}",
            object_id,
            updates_applied,
            txn.store()
          );
        },
        Err(PersistenceError::RecordNotFound(_)) => {
          sync_debug!("collab {} not found in local db", object_id);
        },
        Err(err) => return Err(err),
      }
    }

    if flush {
      let flush_doc = || {
        let encoded = collab.transact().get_encoded_collab_v1();
        ops.flush_doc(
          self.uid,
          &self.workspace_id.to_string(),
          &object_id.to_string(),
          encoded.state_vector.to_vec(),
          encoded.doc_state.to_vec(),
        )?;
        ops.commit_transaction()?;
        Ok::<_, PersistenceError>(())
      };

      if let Err(err) = flush_doc() {
        sync_error!(
          "Failed to flush collab {} to local db: {}",
          collab.object_id(),
          err
        );
      }
    }

    Ok(())
  }

  pub fn remove_doc(&self, object_id: &Uuid) -> Result<(), PersistenceError> {
    let instance = self.inner.get()?;
    let ops = instance.write_txn();
    ops.delete_doc(
      self.uid,
      &self.workspace_id.to_string(),
      &object_id.to_string(),
    )?;
    ops.commit_transaction()?;
    Ok(())
  }

  #[instrument(level = "trace", skip_all, err)]
  pub fn save_update(
    &self,
    object_id: &ObjectId,
    message_id: Option<Rid>,
    update_v1: &[u8],
    action_source: ActionSource,
  ) -> Result<Option<StateVector>, PersistenceError> {
    if update_v1 == Update::EMPTY_V1 {
      sync_trace!("skipping empty update {}", object_id);
      return Ok(None);
    }
    let instance = self.inner.get()?;
    let ops = instance.write_txn();

    sync_trace!(
      "persisting {} update for {}/{} by {}. update: {:#?}",
      action_source,
      self.workspace_id,
      object_id,
      self.uid,
      yrs::Update::decode_v1(update_v1).unwrap(),
    );

    let workspace_id = self.workspace_id.to_string();
    let object_id = object_id.to_string();
    let mut missing = None;
    if ops.is_exist(self.uid, &workspace_id, &object_id) {
      ops.push_update(self.uid, &workspace_id, &object_id, update_v1)?;
    } else {
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
    }
    if let Some(message_id) = message_id {
      ops.update_last_message_id(&self.workspace_id, message_id)?;
    }
    ops.commit_transaction()?;
    Ok(missing)
  }
}

pub trait CollabKVActionExt<'a>: CollabKVAction<'a>
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

    // Keep client IDs 32bit, at least until client ID decoding
    // bug is fixed (see: https://github.com/y-crdt/y-crdt/blob/826d15908105a349eb4a52e327e33cbc4720eda3/yrs/src/updates/decoder.rs#L144)
    let client_id = random::<u32>() as u64;
    sync_trace!(
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
    sync_trace!(
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

/// A holder for RocksDB instances that supports both strong and weak references.
///
/// Since RocksDB should have only one instance at a time, this enum allows for
/// proper reference management. Callers that need to hold a reference to the database
/// should use a weak reference to avoid keeping the instance alive unnecessarily.
#[derive(Clone)]
pub enum DbHolder {
  /// Strong reference to RocksDB instance, maintains the instance alive as long as this reference exists
  Strong(Arc<KVTransactionDBRocksdbImpl>),
  /// Weak reference to RocksDB instance, allows the instance to be dropped when no strong references remain
  Weak(Weak<KVTransactionDBRocksdbImpl>),
}

impl DbHolder {
  /// Attempts to get a strong reference to the RocksDB instance.
  ///
  /// If this is a strong holder, it simply clones the Arc.
  /// If this is a weak holder, it attempts to upgrade the weak reference,
  /// which will fail if the database has been dropped.
  pub fn get(&self) -> anyhow::Result<Arc<KVTransactionDBRocksdbImpl>> {
    match self {
      Self::Strong(db) => Ok(db.clone()),
      Self::Weak(db) => db.upgrade().ok_or_else(|| anyhow!("rocksdb was dropped")),
    }
  }
}

impl From<KVTransactionDBRocksdbImpl> for DbHolder {
  fn from(value: KVTransactionDBRocksdbImpl) -> Self {
    Self::Strong(Arc::new(value))
  }
}

impl From<Weak<KVTransactionDBRocksdbImpl>> for DbHolder {
  fn from(value: Weak<KVTransactionDBRocksdbImpl>) -> Self {
    Self::Weak(value)
  }
}
