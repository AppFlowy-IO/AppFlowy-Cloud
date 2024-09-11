use anyhow::anyhow;
use collab::lock::RwLock;
use collab::preclude::updates::encoder::{Encoder, EncoderV2};
use collab::preclude::{Collab, CollabPlugin, ReadTxn, Snapshot, StateVector, TransactionMut};
use collab_entity::CollabType;
use serde_json::Value;
use sqlx::PgPool;
use std::sync::Arc;

use database::history::ops::get_snapshot_meta_list;
use tonic_proto::history::{RepeatedSnapshotMetaPb, SnapshotMetaPb};

use crate::biz::snapshot::{
  calculate_edit_count, CollabSnapshot, CollabSnapshotState, SnapshotGenerator,
};
use crate::error::HistoryError;

pub struct CollabHistory {
  pub(crate) object_id: String,
  collab: Arc<RwLock<Collab>>,
  collab_type: CollabType,
  snapshot_generator: SnapshotGenerator,
}

impl CollabHistory {
  pub async fn new(object_id: &str, collab: Arc<RwLock<Collab>>, collab_type: CollabType) -> Self {
    let current_edit_count = {
      let read_guard = collab.read().await;
      let txn = read_guard.transact();
      calculate_edit_count(&txn)
    };

    #[cfg(feature = "verbose_log")]
    tracing::trace!(
      "[History] object:{} init edit count: {}",
      object_id,
      current_edit_count
    );

    let snapshot_generator = SnapshotGenerator::new(
      object_id,
      Arc::downgrade(&collab),
      collab_type.clone(),
      current_edit_count as u32,
    );
    collab.read().await.add_plugin(Box::new(CountUpdatePlugin {
      snapshot_generator: snapshot_generator.clone(),
    }));
    collab.write().await.initialize();

    Self {
      object_id: object_id.to_string(),
      snapshot_generator,
      collab,
      collab_type,
    }
  }

  pub async fn generate_snapshot_if_empty(&self) {
    if !self.snapshot_generator.has_snapshot().await {
      self.snapshot_generator.generate().await;
    }
  }

  pub async fn gen_snapshot_context(&self) -> Result<Option<SnapshotContext>, HistoryError> {
    let collab = self.collab.clone();
    let timestamp = chrono::Utc::now().timestamp();
    let snapshots: Vec<CollabSnapshot> = self.snapshot_generator.consume_pending_snapshots().await
          .into_iter()
          // Remove the snapshots which created_at is bigger than the current timestamp
          .filter(|snapshot| snapshot.created_at <= timestamp)
          .collect();

    // If there are no snapshots, we don't need to generate a new snapshot
    if snapshots.is_empty() {
      return Ok(None);
    }
    let collab_type = self.collab_type.clone();
    let object_id = self.object_id.clone();
    let (doc_state, state_vector) = tokio::task::spawn_blocking(move || {
      let lock = collab.blocking_read();
      let result = collab_type.validate_require_data(&lock);
      match result {
        Ok(_) => {
          let txn = lock.transact();
          let doc_state_v2 = txn.encode_state_as_update_v2(&StateVector::default());
          let state_vector = txn.state_vector();
          Ok::<_, HistoryError>((doc_state_v2, state_vector))
        },
        Err(err) => Err::<_, HistoryError>(HistoryError::Internal(anyhow!(
          "Failed to validate {}:{} required data: {}",
          object_id,
          collab_type,
          err
        ))),
      }
    })
    .await
    .map_err(|err| HistoryError::Internal(err.into()))??;

    let collab_type = self.collab_type.clone();
    let object_id = self.object_id.clone();
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
  }

  /// Encode the state of the collab as Update.
  /// We encode the collaboration state as an update using the v2 format, chosen over the v1 format
  /// due to its reduced data size. This optimization helps in minimizing the storage and
  /// transmission overhead, making the process more efficient.
  pub async fn encode_update_v2(&self, snapshot: &Snapshot) -> Result<Vec<u8>, HistoryError> {
    let lock = self.collab.read().await;
    let txn = lock.transact();
    let mut encoder = EncoderV2::new();
    txn
      .encode_state_from_snapshot(snapshot, &mut encoder)
      .map_err(|err| HistoryError::Internal(err.into()))?;
    Ok(encoder.to_vec())
  }

  #[cfg(debug_assertions)]
  pub async fn json(&self) -> Value {
    let lock_guard = self.collab.read().await;
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
  fn receive_update(&self, _object_id: &str, txn: &TransactionMut, _update: &[u8]) {
    self.snapshot_generator.did_apply_update(txn);
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
