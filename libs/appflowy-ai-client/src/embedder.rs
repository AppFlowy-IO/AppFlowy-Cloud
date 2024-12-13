use crate::dto::EmbeddingModel;
use crate::error::AIError;

pub struct Embedder {
  client: ureq::Agent,
  embedding_model: EmbeddingModel,
}

pub const REQUEST_PARALLELISM: usize = 40;

impl Embedder {
  pub fn new() -> Self {
    let client = ureq::AgentBuilder::new()
      .max_idle_connections(REQUEST_PARALLELISM * 2)
      .max_idle_connections_per_host(REQUEST_PARALLELISM * 2)
      .build();

    let embedding_model = EmbeddingModel::TextEmbedding3Small;

    Self {
      client,
      embedding_model,
    }
  }

  pub fn embed(&self, texts: Vec<String>) -> Result<Vec<f32>, AIError> {
    todo!()
  }
}
