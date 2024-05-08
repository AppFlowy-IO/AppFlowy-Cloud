use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SummarizeRowParams {
  pub workspace_id: String,
  pub data: SummarizeRowData,
}

/// Represents different types of content that can be used to summarize a database row.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SummarizeRowData {
  /// Specifies the identity of the row within the database.
  Identity { database_id: String, row_id: String },
  /// Content of the row provided as key-value pairs.
  /// For example:
  /// ```json
  /// {
  ///  "name": "Jack",
  ///  "age": 25,
  ///  "city": "New York"
  /// }
  Content(Map<String, Value>),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SummarizeRowResponse {
  pub text: String,
}
