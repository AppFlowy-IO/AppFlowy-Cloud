use appflowy_ai::client::AppFlowyAIClient;
use serde_json::json;

#[tokio::test]
async fn summarize_row_test() {
  let client = AppFlowyAIClient::new("http://localhost:5001");
  let json = json!({"name": "Jack", "age": 25, "city": "New York"});
  let result = client.summarize_row(json).await.unwrap();
  println!("{:?}", result);
}
