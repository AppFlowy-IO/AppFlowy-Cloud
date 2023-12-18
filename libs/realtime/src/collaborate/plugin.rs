use crate::collaborate::group::CollabGroup;
use crate::entities::RealtimeUser;
use crate::error::RealtimeError;
use app_error::AppError;
use async_trait::async_trait;

use crate::collaborate::{CollabAccessControl, CollabUserId};
use anyhow::anyhow;
use collab::core::awareness::Awareness;
use collab::core::collab::TransactionMutExt;
use collab::core::collab_plugin::EncodedCollabV1;
use collab::core::origin::CollabOrigin;
use collab::core::transaction::DocTransactionExtension;
use collab::preclude::{CollabPlugin, Doc, TransactionMut};
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database_entity::dto::{
  AFAccessLevel, InsertCollabParams, InsertSnapshotParams, QueryCollabParams,
};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, event, info, instrument, trace};

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
    let plugin = Self {
      uid,
      workspace_id,
      storage,
      edit_state,
      group,
      collab_type,
      access_control,
    };
    spawn_period_check(&plugin);
    plugin
  }

  #[instrument(level = "trace", skip_all)]
  async fn insert_new_collab(&self, doc: &Doc, object_id: &str) -> Result<(), AppError> {
    debug!(
      "create new collab, cache full access of {} for user:{}",
      object_id, self.uid
    );

    match doc.get_encoded_collab_v1().encode_to_bytes() {
      Ok(encoded_collab_v1) => {
        let _ = self
          .access_control
          .cache_collab_access_level(
            CollabUserId::from(&self.uid),
            object_id,
            AFAccessLevel::FullAccess,
          )
          .await;

        let params = InsertCollabParams::from_raw_data(
          object_id.to_string(),
          self.collab_type.clone(),
          encoded_collab_v1,
          &self.workspace_id,
        );

        self
          .storage
          .insert_collab(&self.uid, params)
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

/// Spawns an asynchronous task to periodically check and perform flush operations for collaboration groups.
///
/// This function sets up a loop that, at regular intervals, checks if a flush operation is needed
/// based on edit count and time interval. If so, it triggers the flush operation.
///
fn spawn_period_check<S, U, AC>(plugin: &CollabStoragePlugin<S, U, AC>)
where
  S: CollabStorage,
  U: RealtimeUser,
{
  let weak_edit_state = Arc::downgrade(&plugin.edit_state);
  let weak_storage = Arc::downgrade(&plugin.storage);
  let weak_group = plugin.group.clone();
  tokio::spawn(async move {
    let mut interval = interval(Duration::from_secs(30));
    loop {
      interval.tick().await;
      match (
        weak_group.upgrade(),
        weak_storage.upgrade(),
        weak_edit_state.upgrade(),
      ) {
        (Some(group), Some(storage), Some(edit_state)) => {
          let max_edit_count = storage.config().flush_per_update;
          let max_interval = storage.config().flush_per_seconds as i64;
          if edit_state.should_flush(max_edit_count, max_interval) {
            edit_state.flush_edit();
            group.flush_collab();
          }
        },
        _ => break,
      }
    }
  });
}

async fn init_collab_with_raw_data(
  oid: &str,
  encoded_collab: &EncodedCollabV1,
  doc: &Doc,
) -> Result<(), RealtimeError> {
  if encoded_collab.doc_state.is_empty() {
    return Err(RealtimeError::UnexpectedData("doc state is empty"));
  }
  event!(
    tracing::Level::DEBUG,
    "start decoding {}: doc state with len: {}",
    oid,
    encoded_collab.doc_state.len()
  );
  let mut txn = doc.transact_mut();
  let update = Update::decode_v1(&encoded_collab.doc_state)?;
  txn.try_apply_update(update)?;
  event!(tracing::Level::DEBUG, "finish decoding {}: doc state", oid,);
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
    let params = QueryCollabParams {
      object_id: object_id.to_string(),
      workspace_id: self.workspace_id.clone(),
      collab_type: self.collab_type.clone(),
    };

    match self.storage.get_collab_encoded_v1(&self.uid, params).await {
      Ok(encoded_collab_v1) => match init_collab_with_raw_data(object_id, &encoded_collab_v1, doc)
        .await
      {
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
                if let Err(err) = storage.create_snapshot(params).await {
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
          if let Some(encoded_collab_v1) = get_latest_snapshot(object_id, &self.storage).await {
            if let Err(err) = init_collab_with_raw_data(object_id, &encoded_collab_v1, doc).await {
              error!("restore collab with snapshot failed: {:?}", err);
              return;
            }
          }
          error!("init collab failed: {:?}", err)
        },
      },
      Err(err) => match &err {
        AppError::RecordNotFound(_) => {
          // When attempting to retrieve collaboration data from the disk and a 'Record Not Found' error is returned,
          // this indicates that the collaboration is new. Therefore, the current collaboration data should be saved to disk.
          event!(
            tracing::Level::DEBUG,
            "New collab object, insert collab to db"
          );
          if let Err(err) = self.insert_new_collab(doc, object_id).await {
            error!("Insert collab {:?}", err);
          }
        },
        _ => error!("{}", err),
      },
    }
  }
  fn did_init(&self, _awareness: &Awareness, _object_id: &str, _last_sync_at: i64) {
    self.edit_state.set_did_load()
  }

  fn receive_update(&self, _object_id: &str, _txn: &TransactionMut, _update: &[u8]) {
    let count = self.edit_state.increment_edit_count();
    if !self.edit_state.did_load() {
      return;
    }

    if count >= self.storage.config().flush_per_update {
      self.edit_state.flush_edit();
      trace!("number of updates reach flush_per_update, start flushing");
      match self.group.upgrade() {
        None => error!("Group is dropped, skip flush collab"),
        Some(group) => group.flush_collab(),
      }
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

    let params = InsertCollabParams::from_raw_data(
      object_id.to_string(),
      self.collab_type.clone(),
      encoded_collab_v1,
      &self.workspace_id,
    );

    let storage = self.storage.clone();
    let uid = self.uid;

    tokio::spawn(async move {
      info!("[realtime] Flushed collab: {}", params.object_id);
      match storage.insert_collab(&uid, params).await {
        Ok(_) => {},
        Err(err) => error!("Failed to save collab: {:?}", err),
      }
    });
  }
}

async fn get_latest_snapshot<S>(object_id: &str, storage: &S) -> Option<EncodedCollabV1>
where
  S: CollabStorage,
{
  let metas = storage.get_collab_snapshot_list(object_id).await.ok()?;
  let meta = metas.0.first()?;
  let snapshot_data = storage.get_collab_snapshot(&meta.snapshot_id).await.ok()?;
  EncodedCollabV1::decode_from_bytes(&snapshot_data.encoded_collab_v1).ok()
}

struct CollabEditState {
  edit_count: AtomicU32,
  flush_edit_count: AtomicU32,
  flush_interval: AtomicI64,
  did_load_collab: AtomicBool,
}

impl CollabEditState {
  fn new() -> Self {
    Self {
      edit_count: AtomicU32::new(0),
      flush_edit_count: Default::default(),
      flush_interval: AtomicI64::new(chrono::Utc::now().timestamp()),
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

  fn flush_edit(&self) {
    self
      .flush_edit_count
      .store(self.edit_count.load(Ordering::SeqCst), Ordering::SeqCst);
    self
      .flush_interval
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
    // compare current time with last flush time
    let current = chrono::Utc::now().timestamp();
    let prev = self.flush_interval.load(Ordering::SeqCst);
    let current_edit_count = self.edit_count.load(Ordering::SeqCst);
    let prev_flush_edit_count = self.flush_edit_count.load(Ordering::SeqCst);

    if current > prev && current_edit_count != prev_flush_edit_count {
      return current - prev >= max_interval;
    }

    // compare current edit count with last flush edit count
    if current_edit_count > prev_flush_edit_count {
      return current_edit_count - prev_flush_edit_count >= max_edit_count;
    }

    false
  }
}
