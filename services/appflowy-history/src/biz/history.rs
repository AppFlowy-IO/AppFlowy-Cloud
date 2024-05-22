use crate::biz::snapshot::{gen_snapshot, CollabSnapshot, CollabSnapshotState, SnapshotGenerator};

use crate::error::HistoryError;
use collab::core::collab::MutexCollab;
use collab::preclude::updates::encoder::{Encoder, EncoderV2};
use collab::preclude::{CollabPlugin, ReadTxn, Snapshot, StateVector, TransactionMut};
use collab_entity::CollabType;
use database::history::ops::get_snapshot_meta_list;
use serde_json::Value;
use sqlx::PgPool;
use tonic_proto::history::{RepeatedSnapshotMetaPb, SnapshotMetaPb};
use tracing::trace;

pub struct CollabHistory {
  object_id: String,
  mutex_collab: MutexCollab,
  collab_type: CollabType,
  snapshot_generator: SnapshotGenerator,
}

impl CollabHistory {
  pub fn new(
    object_id: &str,
    mutex_collab: MutexCollab,
    collab_type: CollabType,
  ) -> Result<Self, HistoryError> {
    let snapshot_generator =
      SnapshotGenerator::new(object_id, mutex_collab.downgrade(), collab_type.clone());

    mutex_collab.lock().add_plugin(Box::new(CountUpdatePlugin {
      snapshot_generator: snapshot_generator.clone(),
    }));

    Ok(Self {
      object_id: object_id.to_string(),
      snapshot_generator,
      mutex_collab,
      collab_type,
    })
  }

  #[cfg(debug_assertions)]
  /// Generate a snapshot of the current state of the collab
  /// Only for testing purposes. We use [SnapshotGenerator] to generate snapshot
  pub fn gen_snapshot(&self, _uid: i64) -> Result<CollabSnapshot, HistoryError> {
    gen_snapshot(&self.mutex_collab, &self.object_id)
  }

  pub async fn gen_snapshot_context(&self) -> Result<Option<SnapshotContext>, HistoryError> {
    let mutex_collab = self.mutex_collab.clone();
    let snapshot_generator = self.snapshot_generator.clone();
    let object_id = self.object_id.clone();
    let collab_type = self.collab_type.clone();

    tokio::task::spawn_blocking(move || {
      let timestamp = chrono::Utc::now().timestamp();
      let snapshots: Vec<CollabSnapshot> = snapshot_generator.take_pending_snapshots()
          .into_iter()
          // Remove the snapshots which created_at is bigger than the current timestamp
          .filter(|snapshot| snapshot.created_at <= timestamp)
          .collect();

      // If there are no snapshots, we don't need to generate a new snapshot
      if snapshots.is_empty() {
        return Ok(None);
      }
      trace!("[History] prepare to save snapshots to disk");
      let (doc_state, state_vector) = {
        let lock_guard = mutex_collab.lock();
        let txn = lock_guard.try_transaction()?;
        // TODO(nathan): reduce the size of doc_state_v2 by encoding the previous [CollabStateSnapshot] doc_state_v2
        let doc_state_v2 = txn.encode_state_as_update_v2(&StateVector::default());
        let state_vector = txn.state_vector();
        drop(txn);
        (doc_state_v2, state_vector)
      };

      let state = CollabSnapshotState::new(
        object_id,
        doc_state,
        2,
        state_vector,
        chrono::Utc::now().timestamp(),
      );
      Ok(Some(SnapshotContext {
        collab_type,
        state,
        snapshots,
      }))
    })
    .await
    .map_err(|err| HistoryError::Internal(err.into()))?
  }

  /// Encode the state of the collab as Update.
  /// We encode the collaboration state as an update using the v2 format, chosen over the v1 format
  /// due to its reduced data size. This optimization helps in minimizing the storage and
  /// transmission overhead, making the process more efficient.
  pub fn encode_update_v2(&self, snapshot: &Snapshot) -> Result<Vec<u8>, HistoryError> {
    let lock_guard = self.mutex_collab.lock();
    let txn = lock_guard.try_transaction()?;
    let mut encoder = EncoderV2::new();
    txn
      .encode_state_from_snapshot(snapshot, &mut encoder)
      .map_err(|err| HistoryError::Internal(err.into()))?;
    Ok(encoder.to_vec())
  }

  #[cfg(debug_assertions)]
  pub fn json(&self) -> Value {
    let lock_guard = self.mutex_collab.lock();
    lock_guard.to_json_value()
  }
}

pub struct SnapshotContext {
  pub collab_type: CollabType,
  pub state: CollabSnapshotState,
  pub snapshots: Vec<CollabSnapshot>,
}

struct CountUpdatePlugin {
  snapshot_generator: SnapshotGenerator,
}
impl CollabPlugin for CountUpdatePlugin {
  fn receive_update(&self, _object_id: &str, _txn: &TransactionMut, update: &[u8]) {
    self.snapshot_generator.did_apply_update(update);
  }
}

pub async fn get_snapshots(
  object_id: &str,
  collab_type: &CollabType,
  pg_pool: &PgPool,
) -> Result<RepeatedSnapshotMetaPb, HistoryError> {
  let metas = get_snapshot_meta_list(object_id, collab_type, pg_pool)
    .await
    .unwrap();

  let metas = metas
    .into_iter()
    .map(|meta| SnapshotMetaPb {
      oid: meta.oid,
      snapshot: meta.snapshot,
      snapshot_version: meta.snapshot_version,
      created_at: meta.created_at,
    })
    .collect::<Vec<_>>();

  Ok(RepeatedSnapshotMetaPb { items: metas })
}
