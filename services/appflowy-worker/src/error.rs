use collab_importer::error::ImporterError as CollabImporterError;
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
  #[error(transparent)]
  ImportCollabError(#[from] CollabImporterError),

  #[error("Can not open the workspace:{0}")]
  CannotOpenWorkspace(String),

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}

impl ImportError {
  pub fn report(&self) -> (String, String) {
    match self {
      ImportError::ImportCollabError(error) => match error {
        CollabImporterError::InvalidPath(s) => (
          "The provided file path is invalid. Please check the path and try again.".to_string(),
          format!("Invalid path: {}", s),
        ),
        CollabImporterError::InvalidPathFormat => (
          "The file path format is incorrect. Please ensure it is in the correct format."
            .to_string(),
          "Invalid path format".to_string(),
        ),
        CollabImporterError::InvalidFileType(file_type) => (
          "The file type is unsupported. Please use a supported file type.".to_string(),
          format!("Invalid file type: {}", file_type),
        ),
        CollabImporterError::ImportMarkdownError(_) => (
          "There was an issue importing the markdown file. Please verify the file contents."
            .to_string(),
          "Import markdown error".to_string(),
        ),
        CollabImporterError::ImportCsvError(_) => (
          "There was an issue importing the CSV file. Please ensure it is correctly formatted."
            .to_string(),
          "Import CSV error".to_string(),
        ),
        CollabImporterError::ParseMarkdownError(_) => (
          "Failed to parse the markdown file. Please check for any formatting issues.".to_string(),
          "Parse markdown error".to_string(),
        ),
        CollabImporterError::Utf8Error(_) => (
          "There was a character encoding issue. Ensure your file is in UTF-8 format.".to_string(),
          "UTF-8 error".to_string(),
        ),
        CollabImporterError::IOError(_) => (
          "An input/output error occurred. Please check your file and try again.".to_string(),
          "IO error".to_string(),
        ),
        CollabImporterError::FileNotFound => (
          "The specified file could not be found. Please check the file path.".to_string(),
          "File not found".to_string(),
        ),
        CollabImporterError::CannotImport => (
          "The file could not be imported. Please ensure it is in a valid format.".to_string(),
          "Cannot import file".to_string(),
        ),
        CollabImporterError::Internal(_) => (
          "An internal error occurred during the import process. Please try again later."
            .to_string(),
          "Internal error".to_string(),
        ),
      },
      ImportError::CannotOpenWorkspace(err) => (
        "Unable to open the workspace. Please verify the workspace and try again.".to_string(),
        format!("Cannot open workspace: {}", err),
      ),
      ImportError::Internal(err) => (
        "An internal error occurred. Please try again or contact support.".to_string(),
        format!("Internal error: {}", err),
      ),
    }
  }
}
