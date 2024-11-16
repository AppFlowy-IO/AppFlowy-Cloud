pub use collab_importer::error::ImporterError as CollabImporterError;
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

  #[error("S3 service unavailable: {0}")]
  S3ServiceUnavailable(String),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

#[derive(thiserror::Error, Debug)]
pub enum ImportError {
  #[error(transparent)]
  ImportCollabError(#[from] CollabImporterError),

  #[error("Can not open the workspace:{0}")]
  CannotOpenWorkspace(String),

  #[error("Failed to unzip file: {0}")]
  UnZipFileError(String),

  #[error("Upload file not found")]
  UploadFileNotFound,

  #[error("Upload file expired")]
  UploadFileExpire,

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

impl From<WorkerError> for ImportError {
  fn from(err: WorkerError) -> ImportError {
    match err {
      WorkerError::RecordNotFound(_) => ImportError::UploadFileNotFound,
      _ => ImportError::Internal(err.into()),
    }
  }
}

impl ImportError {
  pub fn is_file_not_found(&self) -> bool {
    match self {
      ImportError::ImportCollabError(err) => {
        matches!(err, CollabImporterError::FileNotFound)
      },
      _ => false,
    }
  }
  pub fn report(&self, task_id: &str) -> (String, String) {
    match self {
      ImportError::ImportCollabError(error) => match error {
        CollabImporterError::InvalidPath(s) => (
          format!(
            "Task ID: {} - The provided file path is invalid. Please check the path and try again.",
            task_id
          ),
          format!("Task ID: {} - Invalid path: {}", task_id, s),
        ),
        CollabImporterError::InvalidPathFormat => (
          format!(
            "Task ID: {} - The file path format is incorrect. Please ensure it is in the correct format.",
            task_id
          ),
          format!("Task ID: {} - Invalid path format", task_id),
        ),
        CollabImporterError::InvalidFileType(file_type) => (
          format!(
            "Task ID: {} - The file type is unsupported. Please use a supported file type.",
            task_id
          ),
          format!("Task ID: {} - Invalid file type: {}", task_id, file_type),
        ),
        CollabImporterError::ImportMarkdownError(_) => (
          format!(
            "Task ID: {} - There was an issue importing the markdown file. Please verify the file contents.",
            task_id
          ),
          format!("Task ID: {} - Import markdown error", task_id),
        ),
        CollabImporterError::ImportCsvError(_) => (
          format!(
            "Task ID: {} - There was an issue importing the CSV file. Please ensure it is correctly formatted.",
            task_id
          ),
          format!("Task ID: {} - Import CSV error", task_id),
        ),
        CollabImporterError::ParseMarkdownError(_) => (
          format!(
            "Task ID: {} - Failed to parse the markdown file. Please check for any formatting issues.",
            task_id
          ),
          format!("Task ID: {} - Parse markdown error", task_id),
        ),
        CollabImporterError::Utf8Error(_) => (
          format!(
            "Task ID: {} - There was a character encoding issue. Ensure your file is in UTF-8 format.",
            task_id
          ),
          format!("Task ID: {} - UTF-8 error", task_id),
        ),
        CollabImporterError::IOError(_) => (
          format!(
            "Task ID: {} - An input/output error occurred. Please check your file and try again.",
            task_id
          ),
          format!("Task ID: {} - IO error", task_id),
        ),
        CollabImporterError::FileNotFound => (
          format!(
            "Task ID: {} - The specified file could not be found. Please check the file path.",
            task_id
          ),
          format!("Task ID: {} - File not found", task_id),
        ),
        CollabImporterError::CannotImport => (
          format!(
            "Task ID: {} - The file could not be imported. Please ensure it is in a valid format.",
            task_id
          ),
          format!("Task ID: {} - Cannot import file", task_id),
        ),
        CollabImporterError::Internal(_) => (
          format!(
            "Task ID: {} - An internal error occurred during the import process. Please try again later.",
            task_id
          ),
          format!("Task ID: {} - Internal error", task_id),
        ),
      },
      ImportError::CannotOpenWorkspace(err) => (
        format!(
          "Task ID: {} - Unable to open the workspace. Please verify the workspace and try again.",
          task_id
        ),
        format!("Task ID: {} - Cannot open workspace: {}", task_id, err),
      ),
      ImportError::Internal(err) => (
        format!(
          "Task ID: {} - An internal error occurred. Please try again or contact support.",
          task_id
        ),
        format!("Task ID: {} - Internal error: {}", task_id, err),
      ),
      ImportError::UnZipFileError(_) => {
        (
          format!(
            "Task ID: {} - There was an issue unzipping the file. Please check the file and try again.",
            task_id
          ),
          format!("Task ID: {} - Unzip file error", task_id),
        )
      }
      ImportError::UploadFileNotFound => {
        (
          format!(
            "Task ID: {} - The upload file could not be found. Please check the file and try again.",
            task_id
          ),
          format!("Task ID: {} - Upload file not found", task_id),
        )
      }
      ImportError::UploadFileExpire => {
        (
          format!(
            "Task ID: {} - The upload file has expired. Please upload the file again.",
            task_id
          ),
          format!("Task ID: {} - Upload file expired", task_id),
        )
      }
    }
  }
}
