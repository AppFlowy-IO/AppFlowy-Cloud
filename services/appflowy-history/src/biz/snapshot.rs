use std::ops::Deref;
use std::sync::atomic::AtomicU32;
use std::sync::{Arc, Weak};

use collab::lock::{Mutex, RwLock};
use collab::preclude::updates::encoder::Encode;
use collab::preclude::{Collab, ReadTxn, Snapshot, StateVector};
use collab_entity::CollabType;
use tracing::{trace, warn};

use tonic_proto::history::SnapshotMetaPb;

#[derive(Clone)]
pub struct SnapshotGenerator {
  object_id: String,
  mutex_collab: Weak<RwLock<Collab>>,
  collab_type: CollabType,
  apply_update_count: Arc<AtomicU32>,
  pending_snapshots: Arc<Mutex<Vec<CollabSnapshot>>>,
}

impl SnapshotGenerator {
  pub fn new(object_id: &str, mutex_collab: Weak<RwLock<Collab>>, collab_type: CollabType) -> Self {
    Self {
      object_id: object_id.to_string(),
      mutex_collab,
      collab_type,
      apply_update_count: Default::default(),
      pending_snapshots: Default::default(),
    }
  }

  pub async fn take_pending_snapshots(&self) -> Vec<CollabSnapshot> {
    //FIXME: this should be either a channel or lockless immutable queue
    let mut lock = self.pending_snapshots.lock().await;
    std::mem::take(&mut *lock)
  }

  pub fn did_apply_update(&self, _update: &[u8]) {
    let prev_apply_update_count = self
      .apply_update_count
      .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    // keep it simple for now. we just compare the update count to determine if we need to generate a snapshot.
    // in the future, we can use a more sophisticated algorithm to determine when to generate a snapshot.
    let threshold = gen_snapshot_threshold(&self.collab_type);
    // trace!(
    //   "[History] did_apply_update: object_id={}, current={}, threshold={}",
    //   self.object_id,
    //   prev_apply_update_count,
    //   threshold,
    // );
    if prev_apply_update_count + 1 >= threshold {
      self
        .apply_update_count
        .store(0, std::sync::atomic::Ordering::SeqCst);

      let pending_snapshots = self.pending_snapshots.clone();
      let mutex_collab = self.mutex_collab.clone();
      let object_id = self.object_id.clone();
      tokio::spawn(async move {
        if let Some(collab) = mutex_collab.upgrade() {
          trace!("[History] attempting to generate snapshot");
          let snapshot = gen_snapshot(&*collab.read().await, &object_id);
          trace!("[History] did generate snapshot for {}", snapshot.object_id);
          pending_snapshots.lock().await.push(snapshot);
          warn!("Exceeded maximum retry attempts for snapshot generation");
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
    CollabType::Document => 100,
    CollabType::Database => 20,
    CollabType::WorkspaceDatabase => 20,
    CollabType::Folder => 20,
    CollabType::DatabaseRow => 10,
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
pub fn gen_snapshot(collab: &Collab, object_id: &str) -> CollabSnapshot {
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
