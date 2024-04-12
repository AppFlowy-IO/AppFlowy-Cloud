use crate::biz::snapshot::{CollabSnapshot, CollabSnapshotState};
use crate::error::HistoryError;
use sqlx::PgPool;

pub struct HistoryPersistence {
  pg_pool: PgPool,
}

impl HistoryPersistence {
  pub async fn save_snapshot(
    &self,
    _state: CollabSnapshotState,
    _snapshots: Vec<CollabSnapshot>,
  ) -> Result<(), HistoryError> {
    todo!()
  }
}
