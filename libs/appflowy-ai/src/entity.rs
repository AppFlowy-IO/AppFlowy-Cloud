use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SummarizeRow {
  text: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TranslateRow {
  text: String,
}
