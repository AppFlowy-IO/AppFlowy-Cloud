use arc_swap::ArcSwap;
use collab::lock::{Mutex, RwLock};
use collab::preclude::updates::encoder::Encode;
use collab::preclude::{Collab, ReadTxn, Snapshot, StateVector};
use collab_entity::CollabType;

use std::ops::Deref;

use std::sync::{Arc, Weak};
use tracing::warn;

use tonic_proto::history::SnapshotMetaPb;

#[derive(Clone)]
pub struct SnapshotGenerator {
  object_id: String,
  mutex_collab: Weak<RwLock<Collab>>,
  collab_type: CollabType,
  current_update_count: Arc<ArcSwap<u32>>,
  prev_edit_count: Arc<ArcSwap<u32>>,
  pending_snapshots: Arc<Mutex<Vec<CollabSnapshot>>>,
}

impl SnapshotGenerator {
  pub fn new(
    object_id: &str,
    mutex_collab: Weak<RwLock<Collab>>,
    collab_type: CollabType,
    edit_count: u32,
  ) -> Self {
    Self {
      object_id: object_id.to_string(),
      mutex_collab,
      collab_type,
      current_update_count: Arc::new(ArcSwap::new(Arc::new(edit_count))),
      prev_edit_count: Arc::new(ArcSwap::new(Arc::new(edit_count))),
      pending_snapshots: Default::default(),
    }
  }

  pub async fn consume_pending_snapshots(&self) -> Vec<CollabSnapshot> {
    //FIXME: this should be either a channel or lockless immutable queue
    let mut lock = self.pending_snapshots.lock().await;
    std::mem::take(&mut *lock)
  }

  pub async fn has_snapshot(&self) -> bool {
    !self.pending_snapshots.lock().await.is_empty()
  }

  /// Generate a snapshot if the current edit count is not zero.
  pub async fn generate(&self) {
    if let Some(collab) = self.mutex_collab.upgrade() {
      let current = self.current_update_count.load_full();
      let prev = self.prev_edit_count.load_full();
      if current < prev {
        return;
      }
      let threshold_count = *current - *prev;
      if threshold_count > 10 {
        self
          .prev_edit_count
          .store(self.current_update_count.load_full());

        #[cfg(feature = "verbose_log")]
        tracing::trace!("[History]: object:{} generating snapshot", self.object_id);
        let snapshot = gen_snapshot(
          &*collab.read().await,
          &self.object_id,
          "generate snapshot by periodic tick",
        );
        self.pending_snapshots.lock().await.push(snapshot);
      }
    } else {
      warn!("collab is dropped. cannot generate snapshot")
    }
  }

  pub fn did_apply_update<T: ReadTxn>(&self, txn: &T) {
    let txn_edit_count = calculate_edit_count(txn);
    self
      .current_update_count
      .store(Arc::new(txn_edit_count as u32));

    let current = self.current_update_count.load_full();
    let prev = self.prev_edit_count.load_full();
    if current < prev {
      warn!(
        "object:{} current edit count:{} is less than prev edit count:{}",
        self.object_id, current, prev
      );
      return;
    }
    let threshold_count = *current - *prev;
    let threshold = gen_snapshot_threshold(&self.collab_type);
    #[cfg(feature = "verbose_log")]
    tracing::trace!(
      "[History] object_id:{}, update count:{}, threshold={}",
      self.object_id,
      threshold_count,
      threshold,
    );

    if threshold_count + 1 >= threshold {
      self.prev_edit_count.store(Arc::new(txn_edit_count as u32));
      let pending_snapshots = self.pending_snapshots.clone();
      let mutex_collab = self.mutex_collab.clone();
      let object_id = self.object_id.clone();
      tokio::spawn(async move {
        if let Some(collab) = mutex_collab.upgrade() {
          let snapshot = gen_snapshot(
            &*collab.read().await,
            &object_id,
            &format!("Current edit:{}, threshold:{}", threshold_count, threshold),
          );
          pending_snapshots.lock().await.push(snapshot);
        } else {
          warn!("collab is dropped. cannot generate snapshot")
        }
      });
    }
  }
}

#[inline]
fn gen_snapshot_threshold(collab_type: &CollabType) -> u32 {
  match collab_type {
    CollabType::Document => 500,
    CollabType::Database => 50,
    CollabType::WorkspaceDatabase => 50,
    CollabType::Folder => 50,
    CollabType::DatabaseRow => 50,
    CollabType::UserAwareness => 50,
    CollabType::Unknown => {
      if cfg!(debug_assertions) {
        5
      } else {
        50
      }
    },
  }
}

#[inline]
pub fn gen_snapshot(collab: &Collab, object_id: &str, reason: &str) -> CollabSnapshot {
  tracing::trace!(
    "[History]: generate {} snapshot, reason: {}",
    object_id,
    reason
  );

  let snapshot = collab.transact().snapshot();
  let timestamp = chrono::Utc::now().timestamp();
  CollabSnapshot::new(object_id, snapshot, timestamp)
}

/// Represents the state of a collaborative object (Collab) at a specific timestamp.
/// This is used to revert a Collab to a past state using the closest preceding
/// `CollabStateSnapshot`. When reverting to a specific `CollabSnapshot`,
///
/// locating the nearest `CollabStateSnapshot` whose `created_at` timestamp
/// is less than or equal to the `CollabSnapshot`'s `created_at`.
/// This `CollabStateSnapshot` is then used to restore the Collab's state to the snapshot's timestamp.
pub struct CollabSnapshotState {
  pub snapshot_id: String,
  /// Unique identifier of the collaborative document.
  pub object_id: String,
  /// Binary representation of the Collab's state.
  pub doc_state: Vec<u8>,
  pub doc_state_version: i32,
  pub state_vector: StateVector,
  /// Timestamp indicating when this snapshot was created, measured in milliseconds since the Unix epoch.
  pub created_at: i64,
  /// This field specifies the ID of another snapshot that the current snapshot depends on. If present,
  /// it indicates that the current document's state is built upon or derived from the state of the
  /// specified dependency snapshot.
  pub dependency_snapshot_id: Option<String>,
}

impl CollabSnapshotState {
  pub fn new(
    object_id: String,
    doc_state: Vec<u8>,
    doc_state_version: i32,
    state_vector: StateVector,
    created_at: i64,
  ) -> Self {
    let snapshot_id = uuid::Uuid::new_v4().to_string();
    Self {
      snapshot_id,
      object_id,
      doc_state,
      doc_state_version,
      state_vector,
      created_at,
      dependency_snapshot_id: None,
    }
  }
}

/// Captures a significant version of a collaborative object (Collab), marking a specific point in time.
/// This snapshot is identified by a unique ID and linked to a specific `CollabStateSnapshot`.
/// It represents a milestone or version of the Collab that can be referenced or reverted to.
pub struct CollabSnapshot {
  pub object_id: String,
  /// Snapshot data capturing the Collab's state at the time of the snapshot.
  pub snapshot: Snapshot,
  /// Timestamp indicating when this snapshot was created, measured in milliseconds since the Unix epoch.
  pub created_at: i64,
}
impl Deref for CollabSnapshot {
  type Target = Snapshot;

  fn deref(&self) -> &Self::Target {
    &self.snapshot
  }
}

impl CollabSnapshot {
  pub fn new(object_id: &str, snapshot: Snapshot, created_at: i64) -> Self {
    Self {
      snapshot,
      object_id: object_id.to_string(),
      created_at,
    }
  }
}

impl From<CollabSnapshot> for SnapshotMetaPb {
  fn from(snapshot: CollabSnapshot) -> Self {
    let snapshot_data = snapshot.encode_v1();
    Self {
      oid: snapshot.object_id,
      snapshot: snapshot_data,
      snapshot_version: 1,
      created_at: snapshot.created_at,
    }
  }
}

#[inline]
pub(crate) fn calculate_edit_count<T: ReadTxn>(txn: &T) -> u64 {
  let snapshot = txn.snapshot();
  let mut insert_count = 0;
  for (_, &clock) in snapshot.state_map.iter() {
    insert_count += clock as u64;
  }

  let mut delete_count = 0;
  for (_, range) in snapshot.delete_set.iter() {
    for f in range.iter() {
      let deleted_segments = f.len() as u64;
      delete_count += deleted_segments;
    }
  }

  insert_count + delete_count
}
