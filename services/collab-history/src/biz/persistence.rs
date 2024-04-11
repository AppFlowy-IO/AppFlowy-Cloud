use crate::biz::snapshot::{CollabSnapshot, CollabStateSnapshot};
use crate::error::HistoryError;
use sqlx::PgPool;

pub struct HistoryPersistence {
  pg_pool: PgPool,
}

impl HistoryPersistence {
  pub async fn save_snapshot(
    &self,
    state: CollabStateSnapshot,
    snapshots: Vec<CollabSnapshot>,
  ) -> Result<(), HistoryError> {
    todo!()
  }
}
