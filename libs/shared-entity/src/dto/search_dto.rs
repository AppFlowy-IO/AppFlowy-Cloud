use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_repr::{Deserialize_repr, Serialize_repr};
use uuid::Uuid;

/// Parameters used to customize the collab vector search query.
/// In response, a list of [SearchDocumentResponseItem] is returned.
#[derive(Clone, Debug, Deserialize)]
pub struct SearchDocumentRequest {
  /// Query statement to search for.
  pub query: String,
  /// Maximum number of results to return. Default: 10.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub limit: Option<u32>,
  /// Maximum length of the content string preview to return. Default: 180.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub preview_size: Option<u32>,

  #[serde(default = "default_only_context")]
  pub only_context: bool,

  #[serde(default = "default_search_score_limit")]
  pub score_limit: f32,
}

fn default_search_score_limit() -> f32 {
  // The limit of what the score should be for results, used to
  // filter out irrelevant results.
  // https://community.openai.com/t/rule-of-thumb-cosine-similarity-thresholds/693670/5
  0.3
}

fn default_only_context() -> bool {
  true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Summary {
  pub content: String,
  pub metadata: Value,
  pub score: String,
}

/// Response array element for the collab vector search query.
/// See: [SearchDocumentRequest].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchResult {
  pub summaries: Vec<Summary>,
  pub items: Vec<SearchDocumentResponseItem>,
}

/// Response array element for the collab vector search query.
/// See: [SearchDocumentRequest].
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SearchDocumentResponseItem {
  /// Unique object identifier.
  pub object_id: Uuid,
  /// Workspace, result object belongs to.
  pub workspace_id: Uuid,
  /// Match score of this search result to an original query. Score represents cosine distance
  /// between the query and the document embedding [-1.0..1.0]. The higher, the better.
  /// List of results is sorted by this value by default.
  pub score: f64,
  /// Type of the content to be presented in preview field. This is a hint what
  /// kind of content was used to match the user query ie. document plain text, pdf attachment etc.
  pub content_type: Option<SearchContentType>,
  /// First N characters of the indexed content matching the user query. It doesn't have to contain
  /// the user query itself.
  pub preview: Option<String>,
  /// Name of the user who created/own the document.
  pub created_by: String,
  /// Date when the document was created.
  pub created_at: DateTime<Utc>,
}

/// Type of the document content to be presented in the search results.
/// See: [SearchDocumentResponseItem].
#[repr(i32)]
#[derive(Clone, Copy, Debug, Serialize_repr, Deserialize_repr)]
pub enum SearchContentType {
  /// Document block contents displayed as plain text.
  PlainText = 0,
}

impl SearchContentType {
  /// Converts the database content type (an integer) into current [SearchContentType].
  pub fn from_record(content_type: i32) -> Option<Self> {
    match content_type {
      0 => Some(SearchContentType::PlainText),
      _ => None,
    }
  }
}
