#[derive(thiserror::Error, Debug)]
pub enum WorkerError {
  #[error(transparent)]
  ZipError(#[from] async_zip::error::ZipError),

  #[error("Record not found: {0}")]
  RecordNotFound(String),

  #[error(transparent)]
  IOError(#[from] std::io::Error),

  #[error(transparent)]
  ImportError(#[from] ImportError),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum ImportError {
  #[error("Can not open the imported workspace: {0}")]
  OpenImportWorkspaceError(String),

  #[error(transparent)]
  ImportCollabError(#[from] collab_importer::error::ImporterError),

  #[error("Can not open the workspace:{0}")]
  CannotOpenWorkspace(String),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}
