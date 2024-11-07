use crate::appflowy_ai_client;

use appflowy_ai_client::dto::{AIModel, TranslateItem, TranslateRowData};

#[tokio::test]
async fn translate_row_test() {
  let client = appflowy_ai_client();

  let mut cells = Vec::new();
  for (key, value) in [("book name", "Atomic Habits"), ("author", "James Clear")].iter() {
    cells.push(TranslateItem {
      title: key.to_string(),
      content: value.to_string(),
    });
  }

  let data = TranslateRowData {
    cells,
    language: "Chinese".to_string(),
    include_header: false,
  };

  let result = client
    .translate_row(data, AIModel::GPT4oMini)
    .await
    .unwrap();
  assert_eq!(result.items.len(), 2);
}
