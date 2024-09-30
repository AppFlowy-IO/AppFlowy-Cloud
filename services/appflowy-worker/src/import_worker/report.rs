use axum::async_trait;

#[async_trait]
pub trait ImportNotifier: Send {
  async fn notify_progress(&self, progress: ImportProgress);
}

pub enum ImportProgress {
  Started { workspace_id: String },
  Finished(ImportResult),
}

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
