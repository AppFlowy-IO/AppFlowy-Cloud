use appflowy_ai_client::client::AppFlowyAIClient;

mod chat_test;
mod row_test;

// mod index_test;

pub fn appflowy_ai_client() -> AppFlowyAIClient {
  AppFlowyAIClient::new("http://localhost:5001")
}
