
use axum::async_trait;

#[async_trait]
pub trait ImportNotifier: Send + Sync + 'static {
  async fn notify_progress(&self, progress: ImportProgress);
}

#[derive(Debug, Clone)]
pub enum ImportProgress {
  Started { workspace_id: String },
  Finished(ImportResult),
}

#[derive(Debug, Clone)]
pub struct ImportResult {
  pub user_name: String,
  pub user_email: String,
  pub is_success: bool,
  pub value: serde_json::Value,
}
