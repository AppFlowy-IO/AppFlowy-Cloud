use crate::appflowy_ai_client;
use std::collections::HashMap;

use appflowy_ai_client::dto::TranslateRowData;

#[tokio::test]
async fn translate_row_test() {
  let client = appflowy_ai_client();

  let mut cells = HashMap::new();
  for (key, value) in [("book name", "Atomic Habits"), ("author", "James Clear")].iter() {
    cells.insert(key.to_string(), value.to_string());
  }

  let data = TranslateRowData {
    cells,
    language: "Chinese".to_string(),
    include_header: false,
  };

  let result = client.translate_row(data).await.unwrap();
  assert_eq!(result.items.len(), 2);
}
