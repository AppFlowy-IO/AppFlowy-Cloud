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
  pub workspace_id: String,
}

pub struct ImportResultBuilder {
  workspace_id: String,
}

impl ImportResultBuilder {
  pub fn new(workspace_id: String) -> Self {
    Self { workspace_id }
  }

  pub fn build(self) -> ImportResult {
    ImportResult {
      workspace_id: self.workspace_id,
    }
  }
}
