use crate::indexer::vector::open_ai;
use app_error::AppError;
use appflowy_ai_client::dto::{EmbeddingModel, EmbeddingRequest, OpenAIEmbeddingResponse};

#[derive(Debug, Clone)]
pub enum Embedder {
  OpenAI(open_ai::Embedder),
}

impl Embedder {
  pub fn embed(&self, params: EmbeddingRequest) -> Result<OpenAIEmbeddingResponse, AppError> {
    match self {
      Self::OpenAI(embedder) => embedder.embed(params),
    }
  }

  pub fn model(&self) -> EmbeddingModel {
    EmbeddingModel::TextEmbedding3Small
  }
}
