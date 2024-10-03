use crate::import_worker::report::{ImportNotifier, ImportProgress};
use axum::async_trait;

pub struct EmailNotifier;

#[async_trait]
impl ImportNotifier for EmailNotifier {
  async fn notify_progress(&self, progress: ImportProgress) {
    match progress {
      ImportProgress::Started { workspace_id: _ } => {},
      ImportProgress::Finished(_result) => {},
    }
  }
}
