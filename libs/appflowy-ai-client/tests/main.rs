use appflowy_ai_client::client::AppFlowyAIClient;

mod chat_test;
mod row_test;

// mod index_test;

pub fn appflowy_ai_client() -> AppFlowyAIClient {
  let url =
    std::env::var("APPFLOWY_AI_URI").unwrap_or_else(|_| "http://localhost:5001".to_string());
  AppFlowyAIClient::new(&url)
}
