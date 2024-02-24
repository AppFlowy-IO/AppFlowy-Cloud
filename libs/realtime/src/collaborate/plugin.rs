use crate::entities::RealtimeUser;
use crate::error::RealtimeError;
use app_error::AppError;
use async_trait::async_trait;
use std::fmt::Display;

use crate::collaborate::CollabAccessControl;
use anyhow::anyhow;

use collab::core::collab::TransactionMutExt;
use collab::core::collab_plugin::EncodedCollab;
use collab::core::origin::CollabOrigin;
use collab::core::transaction::DocTransactionExtension;
use collab::preclude::{Collab, CollabPlugin, Doc, TransactionMut};
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database_entity::dto::{
  AFAccessLevel, CreateCollabParams, InsertSnapshotParams, QueryCollabParams,
};
use md5::Digest;
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Weak};

use tracing::{debug, error, event, info, instrument, trace, warn};

use crate::collaborate::group_control::CollabGroup;
use yrs::updates::decoder::Decode;
use yrs::{Transact, Update};

pub struct CollabStoragePlugin<S, U, AC> {
  uid: i64,
  workspace_id: String,
  storage: Arc<S>,
  edit_state: Arc<CollabEditState>,
  group: Weak<CollabGroup<U>>,
  collab_type: CollabType,
  access_control: Arc<AC>,
  latest_collab_md5: Mutex<Option<Digest>>,
}

impl<S, U, AC> CollabStoragePlugin<S, U, AC>
where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
{
  pub fn new(
    uid: i64,
    workspace_id: &str,
    collab_type: CollabType,
    storage: S,
    group: Weak<CollabGroup<U>>,
    access_control: Arc<AC>,
  ) -> Self {
    let storage = Arc::new(storage);
    let workspace_id = workspace_id.to_string();
    let edit_state = Arc::new(CollabEditState::new());
    Self {
      uid,
      workspace_id,
      storage,
      edit_state,
      group,
      collab_type,
      access_control,
      latest_collab_md5: Default::default(),
    }
  }

  #[instrument(level = "info", skip(self,doc), err, fields(object_id = %object_id))]
  async fn insert_new_collab(&self, doc: &Doc, object_id: &str) -> Result<(), AppError> {
    match doc.get_encoded_collab_v1().encode_to_bytes() {
      Ok(encoded_collab_v1) => {
        let _ = self
          .access_control
          .insert_collab_access_level(&self.uid, object_id, AFAccessLevel::FullAccess)
          .await;

        let params = CreateCollabParams {
          object_id: object_id.to_string(),
          encoded_collab_v1,
          collab_type: self.collab_type.clone(),
          override_if_exist: false,
          workspace_id: self.workspace_id.clone(),
        };

        self
          .storage
          .upsert_collab(&self.uid, params)
          .await
          .map_err(|err| {
            error!("fail to create new collab in plugin: {:?}", err);
            err
          })
      },
      Err(err) => Err(AppError::Internal(anyhow!(
        "fail to encode doc to bytes: {:?}",
        err
      ))),
    }
  }
}

async fn init_collab(
  oid: &str,
  encoded_collab: &EncodedCollab,
  doc: &Doc,
) -> Result<(), RealtimeError> {
  if encoded_collab.doc_state.is_empty() {
    return Err(RealtimeError::UnexpectedData("doc state is empty"));
  }

  // Can turn off INFO level into DEBUG. For now, just to see the log
  event!(
    tracing::Level::DEBUG,
    "start decoding:{} state len: {}, sv len: {}, v: {:?}",
    oid,
    encoded_collab.doc_state.len(),
    encoded_collab.state_vector.len(),
    encoded_collab.version
  );
  let update = Update::decode_v1(&encoded_collab.doc_state)?;
  let mut txn = doc.transact_mut();
  txn.try_apply_update(update)?;
  drop(txn);

  Ok(())
}

#[async_trait]
impl<S, U, AC> CollabPlugin for CollabStoragePlugin<S, U, AC>
where
  S: CollabStorage,
  U: RealtimeUser,
  AC: CollabAccessControl,
{
  async fn init(&self, object_id: &str, _origin: &CollabOrigin, doc: &Doc) {
    let params = QueryCollabParams::new(object_id, self.collab_type.clone(), &self.workspace_id);
    match self.storage.get_collab_encoded(&self.uid, params).await {
      Ok(encoded_collab_v1) => match init_collab(object_id, &encoded_collab_v1, doc).await {
        Ok(_) => {
          // Attempt to create a snapshot for the collaboration object. When creating this snapshot, it is
          // assumed that the 'encoded_collab_v1' is already in a valid format. Therefore, there is no need
          // to verify the outcome of the 'encode_to_bytes' operation.
          if self.storage.should_create_snapshot(object_id).await {
            let cloned_workspace_id = self.workspace_id.clone();
            let cloned_object_id = object_id.to_string();
            let storage = self.storage.clone();

            event!(tracing::Level::DEBUG, "Creating collab snapshot");
            let _ = tokio::task::spawn_blocking(move || {
              let params = InsertSnapshotParams {
                object_id: cloned_object_id,
                encoded_collab_v1: encoded_collab_v1.encode_to_bytes().unwrap(),
                workspace_id: cloned_workspace_id,
              };

              tokio::spawn(async move {
                // FIXME(nathan): There is a potential issue when concurrently spawning tasks to create snapshots. A subsequent
                // task for creating a snapshot might write to the database before a previous task completes. To address
                // this, consider using `stream!` to queue these tasks, ensuring they are executed in the order they were
                // spawned.
                if let Err(err) = storage.queue_snapshot(params).await {
                  error!("create snapshot {:?}", err);
                }
              });
            })
            .await;
          }
        },
        Err(err) => {
          // When initializing a collaboration object, if the 'init_collab_with_raw_data' operation fails, attempt to
          // restore the collaboration object from the latest snapshot.
          error!(
            "init collab:{} error: {:?}, try to restore from snapshot",
            object_id, err
          );

          match get_latest_snapshot(&self.workspace_id, object_id, &self.storage).await {
            None => error!("No snapshot found for collab: {}", object_id),
            Some(encoded_collab) => match init_collab(object_id, &encoded_collab, doc).await {
              Ok(_) => info!("restore collab:{} with snapshot success", object_id),
              Err(err) => {
                error!(
                  "restore collab:{} with snapshot failed: {:?}",
                  object_id, err
                );
              },
            },
          }
        },
      },
      Err(err) => match &err {
        AppError::RecordNotFound(_) => {
          // When attempting to retrieve collaboration data from the disk and a 'Record Not Found' error is returned,
          // this indicates that the collaboration is new. Therefore, the current collaboration data should be saved to disk.
          event!(
            tracing::Level::INFO,
            "Create new collab:{} from realtime editing",
            object_id
          );
          if let Err(err) = self.insert_new_collab(doc, object_id).await {
            error!("Insert collab {:?}", err);
          }
        },
        _ => error!("{}", err),
      },
    }
  }
  fn did_init(&self, _collab: &Collab, _object_id: &str, _last_sync_at: i64) {
    self.edit_state.set_did_load()
  }

  fn receive_update(&self, object_id: &str, _txn: &TransactionMut, _update: &[u8]) {
    let _ = self.edit_state.increment_edit_count();
    if !self.edit_state.did_load() {
      return;
    }

    trace!("{} edit state:{}", object_id, self.edit_state);
    if self
      .edit_state
      .should_flush(self.storage.config().flush_per_update, 3 * 60)
    {
      self.edit_state.tick();

      let object_id = object_id.to_string();
      let weak_group = self.group.clone();
      tokio::spawn(async move {
        match weak_group.upgrade() {
          None => warn!("{}: Group is dropped, skip flush collab", object_id),
          Some(group) => {
            info!(
              "{}: number of updates reach flush_per_update, start flushing",
              object_id
            );
            group.flush_collab().await;
          },
        }
      });
    }
  }

  fn flush(&self, object_id: &str, doc: &Doc) {
    let encoded_collab_v1 = match doc.get_encoded_collab_v1().encode_to_bytes() {
      Ok(data) => data,
      Err(err) => {
        error!("Error encoding: {:?}", err);
        return;
      },
    };

    // compare the current encoded collab md5 with the latest one, if they are the same, skip the flush.
    // TODO(nathan): compress the encoded collab to reduce disk usage.
    let digest = md5::compute(&encoded_collab_v1);
    if self.latest_collab_md5.lock().as_ref() == Some(&digest) {
      debug!(
        "skip flush collab:{} because the content is the same",
        object_id
      );
      return;
    }
    *self.latest_collab_md5.lock() = Some(digest);

    // Insert the encoded collab into the database
    let params = CreateCollabParams {
      object_id: object_id.to_string(),
      encoded_collab_v1,
      collab_type: self.collab_type.clone(),
      override_if_exist: false,
      workspace_id: self.workspace_id.clone(),
    };

    let storage = self.storage.clone();
    let uid = self.uid;
    tokio::spawn(async move {
      info!("[realtime] flush collab: {}", params.object_id);
      match storage.upsert_collab(&uid, params).await {
        Ok(_) => {},
        Err(err) => error!("Failed to save collab: {:?}", err),
      }
    });
  }
}

async fn get_latest_snapshot<S>(
  workspace_id: &str,
  object_id: &str,
  storage: &S,
) -> Option<EncodedCollab>
where
  S: CollabStorage,
{
  let metas = storage.get_collab_snapshot_list(object_id).await.ok()?;
  let meta = metas.0.first()?;
  let snapshot_data = storage
    .get_collab_snapshot(workspace_id, &meta.object_id, &meta.snapshot_id)
    .await
    .ok()?;
  EncodedCollab::decode_from_bytes(&snapshot_data.encoded_collab_v1).ok()
}

struct CollabEditState {
  edit_count: AtomicU32,
  prev_edit_count: AtomicU32,
  prev_flush_timestamp: AtomicI64,
  did_load_collab: AtomicBool,
}

impl Display for CollabEditState {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(
          f,
          "CollabEditState {{ edit_count: {}, prev_edit_count: {}, prev_flush_timestamp: {}, did_load_collab: {} }}",
          self.edit_count.load(Ordering::SeqCst),
          self.prev_edit_count.load(Ordering::SeqCst),
          self.prev_flush_timestamp.load(Ordering::SeqCst),
          self.did_load_collab.load(Ordering::SeqCst)
        )
  }
}

impl CollabEditState {
  fn new() -> Self {
    Self {
      edit_count: AtomicU32::new(0),
      prev_edit_count: Default::default(),
      prev_flush_timestamp: AtomicI64::new(chrono::Utc::now().timestamp()),
      did_load_collab: AtomicBool::new(false),
    }
  }

  fn set_did_load(&self) {
    self.did_load_collab.store(true, Ordering::SeqCst);
  }

  fn did_load(&self) -> bool {
    self.did_load_collab.load(Ordering::SeqCst)
  }

  fn increment_edit_count(&self) -> u32 {
    self.edit_count.fetch_add(1, Ordering::SeqCst)
  }

  fn tick(&self) {
    self
      .prev_edit_count
      .store(self.edit_count.load(Ordering::SeqCst), Ordering::SeqCst);
    self
      .prev_flush_timestamp
      .store(chrono::Utc::now().timestamp(), Ordering::SeqCst);
  }

  /// Determines whether a flush operation should be performed based on edit count and time interval.
  ///
  /// This function checks two conditions to decide if flushing is necessary:
  /// 1. Time-based: Compares the current time with the last flush time. A flush is needed if the time
  /// elapsed since the last flush is greater than or equal to `max_interval`.
  ///
  /// 2. Edit count-based: Compares the current edit count with the last flush edit count. A flush is
  /// required if the number of new edits since the last flush is greater than or equal to `max_edit_count`.
  ///
  /// # Arguments
  /// * `max_edit_count` - The maximum number of edits allowed before a flush is triggered.
  /// * `max_interval` - The maximum time interval (in seconds) allowed before a flush is triggered.
  fn should_flush(&self, max_edit_count: u32, max_interval: i64) -> bool {
    let current_edit_count = self.edit_count.load(Ordering::SeqCst);
    let prev_edit_count = self.prev_edit_count.load(Ordering::SeqCst);

    // compare current edit count with last flush edit count
    if current_edit_count > prev_edit_count {
      return (current_edit_count - prev_edit_count) >= max_edit_count;
    }

    let now = chrono::Utc::now().timestamp();
    let prev = self.prev_flush_timestamp.load(Ordering::SeqCst);
    if now > prev && current_edit_count != prev_edit_count {
      return now - prev >= max_interval;
    }

    false
  }
}
