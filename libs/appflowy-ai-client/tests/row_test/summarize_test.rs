use crate::appflowy_ai_client;

use serde_json::json;

#[tokio::test]
async fn summarize_row_test() {
  let client = appflowy_ai_client();
  let json = json!({"name": "Jack", "age": 25, "city": "New York"});

  let result = client
    .summarize_row(json.as_object().unwrap())
    .await
    .unwrap();
  result.text.contains("Jack");
  result.text.contains("New York");
  println!("{:?}", result);
}
