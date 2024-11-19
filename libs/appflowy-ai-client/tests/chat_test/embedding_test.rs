use crate::appflowy_ai_client;

use appflowy_ai_client::dto::{
  EmbeddingEncodingFormat, EmbeddingInput, EmbeddingModel, EmbeddingRequest,
};

#[tokio::test]
async fn embedding_test() {
  let client = appflowy_ai_client();
  let request = EmbeddingRequest {
    input: EmbeddingInput::String("hello world".to_string()),
    model: EmbeddingModel::TextEmbedding3Small.to_string(),
    chunk_size: 1000,
    encoding_format: EmbeddingEncodingFormat::Float,
    dimensions: EmbeddingModel::TextEmbedding3Small.default_dimensions(),
  };
  let result = client.embeddings(request).await.unwrap();
  assert!(result.total_tokens > 0);
  assert!(!result.data.is_empty());
}
