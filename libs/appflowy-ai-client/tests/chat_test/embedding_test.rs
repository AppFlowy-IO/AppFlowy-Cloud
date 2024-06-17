use crate::appflowy_ai_client;

use appflowy_ai_client::dto::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingRequest, EmbeddingsModel,
};

#[tokio::test]
async fn embedding_test() {
  let client = appflowy_ai_client();
  let request = EmbeddingRequest {
    input: EmbeddingInput::String("hello world".to_string()),
    model: EmbeddingsModel::TextEmbedding3Small.to_string(),
    chunk_size: 1000,
    encoding_format: EmbeddingEncodingFormat::Float,
    dimensions: 1536,
  };
  let result = client.embeddings(request).await.unwrap();
  assert!(result.total_tokens > 0);
  assert!(!result.data.is_empty());
}
