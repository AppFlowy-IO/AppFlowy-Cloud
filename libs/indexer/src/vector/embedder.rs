use crate::vector::open_ai;
use crate::vector::open_ai::async_embed;
use app_error::AppError;
use appflowy_ai_client::dto::EmbeddingModel;
pub use async_openai::config::{AzureConfig, OpenAIConfig};
pub use async_openai::types::{
  CreateEmbeddingRequest, CreateEmbeddingRequestArgs, CreateEmbeddingResponse, EmbeddingInput,
  EncodingFormat,
};
use infra::env_util::get_env_var_opt;

#[derive(Debug, Clone)]
pub enum AFEmbedder {
  OpenAI(open_ai::OpenAIEmbedder),
  AzureOpenAI(open_ai::AzureOpenAIEmbedder),
}

impl AFEmbedder {
  pub async fn async_embed(
    &self,
    params: CreateEmbeddingRequest,
  ) -> Result<CreateEmbeddingResponse, AppError> {
    match self {
      Self::OpenAI(embedder) => async_embed(&embedder.client, params).await,
      Self::AzureOpenAI(embedder) => async_embed(&embedder.client, params).await,
    }
  }

  pub fn model(&self) -> EmbeddingModel {
    EmbeddingModel::default_model()
  }
}

pub fn open_ai_config() -> Option<OpenAIConfig> {
  get_env_var_opt("AI_OPENAI_API_KEY").map(|v| OpenAIConfig::default().with_api_key(v))
}

pub fn azure_open_ai_config() -> Option<AzureConfig> {
  let azure_open_ai_api_key = get_env_var_opt("AI_AZURE_OPENAI_API_KEY")?;
  let azure_open_ai_api_base = get_env_var_opt("AI_AZURE_OPENAI_API_BASE")?;
  let azure_open_ai_api_version = get_env_var_opt("AI_AZURE_OPENAI_API_VERSION")?;

  Some(
    AzureConfig::new()
      .with_api_key(azure_open_ai_api_key)
      .with_api_base(azure_open_ai_api_base)
      .with_api_version(azure_open_ai_api_version),
  )
}
