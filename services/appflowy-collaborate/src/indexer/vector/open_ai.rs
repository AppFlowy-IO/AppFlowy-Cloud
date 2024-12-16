use crate::indexer::vector::rest::check_response;
use anyhow::anyhow;
use app_error::AppError;
use appflowy_ai_client::dto::{EmbeddingRequest, OpenAIEmbeddingResponse};
use serde::de::DeserializeOwned;
use std::time::Duration;

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

  pub fn embed(&self, params: EmbeddingRequest) -> Result<OpenAIEmbeddingResponse, AppError> {
    for attempt in 0..3 {
      let request = self
        .client
        .post(OPENAI_EMBEDDINGS_URL)
        .set("Authorization", &self.bearer)
        .set("Content-Type", "application/json");

      let result = check_response(request.send_json(&params));
      let retry_duration = match result {
        Ok(response) => {
          let data = from_response::<OpenAIEmbeddingResponse>(response)?;
          return Ok(data);
        },
        Err(retry) => retry.into_duration(attempt),
      }
      .map_err(|err| AppError::Internal(err.into()))?;
      let retry_duration = retry_duration.min(Duration::from_secs(10));
      std::thread::sleep(retry_duration);
    }

    Err(AppError::Internal(anyhow!(
      "Failed to generate embeddings after 3 attempts"
    )))
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
