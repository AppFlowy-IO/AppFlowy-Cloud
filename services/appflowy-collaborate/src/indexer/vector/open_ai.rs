use app_error::AppError;
use appflowy_ai_client::dto::{EmbeddingRequest, EmbeddingResponse};
use serde::de::DeserializeOwned;

pub const OPENAI_EMBEDDINGS_URL: &str = "https://api.openai.com/v1/embeddings";

pub const REQUEST_PARALLELISM: usize = 40;

#[derive(Debug, Clone)]
pub struct Embedder {
  bearer: String,
  client: ureq::Agent,
}

impl Embedder {
  pub fn new(api_key: String) -> Self {
    let bearer = format!("Bearer {api_key}");
    let client = ureq::AgentBuilder::new()
      .max_idle_connections(REQUEST_PARALLELISM * 2)
      .max_idle_connections_per_host(REQUEST_PARALLELISM * 2)
      .build();

    Self { bearer, client }
  }

  pub fn embed(&self, params: EmbeddingRequest) -> Result<EmbeddingResponse, AppError> {
    let request = self
      .client
      .post(OPENAI_EMBEDDINGS_URL)
      .set("Authorization", &self.bearer)
      .set("Content-Type", "application/json");

    let response = request
      .send_json(params)
      .map_err(|err| AppError::InvalidRequest(err.to_string()))?;
    let data = from_response::<EmbeddingResponse>(response)?;
    Ok(data)
  }
}

pub fn from_response<T>(resp: ureq::Response) -> Result<T, anyhow::Error>
where
  T: DeserializeOwned,
{
  let status_code = resp.status();
  if status_code != 200 {
    let body = resp.into_string()?;
    anyhow::bail!("error code: {}, {}", status_code, body)
  }

  let resp = resp.into_json()?;
  Ok(resp)
}
