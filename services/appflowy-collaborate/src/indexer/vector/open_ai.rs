use app_error::AppError;
use appflowy_ai_client::client::AIResponse;
use appflowy_ai_client::dto::{EmbeddingRequest, OpenAIEmbeddingResponse};
use appflowy_ai_client::error::AIError;
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
    const MAX_RETRIES: u32 = 2;
    const RETRY_DELAY: Duration = Duration::from_secs(2);
    let mut retries = 0;

    loop {
      let request = self
        .client
        .post(OPENAI_EMBEDDINGS_URL)
        .set("Authorization", &self.bearer)
        .set("Content-Type", "application/json");

      match request.send_json(&params) {
        Ok(response) => {
          let data = from_response::<OpenAIEmbeddingResponse>(response)?;
          return Ok(data);
        },
        Err(err) => {
          if matches!(err, ureq::Error::Transport(_)) {
            return Err(AppError::InvalidRequest(err.to_string()));
          }

          retries += 1;
          if retries >= MAX_RETRIES {
            tracing::error!(
              "embeddings request failed after {} retries: {:?}",
              retries,
              err
            );
            return Err(AppError::Internal(err.into()));
          }
          std::thread::sleep(RETRY_DELAY);
        },
      }
    }
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
