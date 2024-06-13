use crate::appflowy_ai_client;

use appflowy_ai_client::dto::{TagItem, TagRowData};

#[tokio::test]
async fn translate_row_test() {
  let client = appflowy_ai_client();

  let mut items = Vec::new();
  for (key, value) in [("book name", "Atomic Habits"), ("author", "James Clear")].iter() {
    items.push(TagItem {
      title: key.to_string(),
      content: value.to_string(),
    });
  }

  let data = TagRowData {
    existing_tags: vec![],
    items,
    num_tags: 3,
  };

  let result = client.tag_row(data).await.unwrap();
  assert_eq!(result.tags.len(), 3);
}
