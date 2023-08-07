use anyhow::{anyhow, Error};

pub struct Client {
  http_client: reqwest::Client,
  base_url: String,
}

impl Client {
  pub fn from(c: reqwest::Client, base_url: &str) -> Self {
    Self {
      base_url: base_url.to_string(),
      http_client: c,
    }
  }

  pub async fn register(&self, name: &str, email: &str, password: &str) -> Result<String, Error> {
    let url = format!("{}/api/user/register", self.base_url);
    let payload = serde_json::json!({
        "name": name,
        "password": password,
        "email": email,
    });
    let resp = self.http_client.post(&url).json(&payload).send().await?;
    let token: Token = from(resp).await?;
    Ok(token.token)
  }
}

async fn from<T>(resp: reqwest::Response) -> Result<T, Error>
where
  T: serde::de::DeserializeOwned,
{
  let status_code = resp.status();
  if !status_code.is_success() {
    let body = resp.text().await?;
    anyhow::bail!("got error code: {}, body: {}", status_code, body)
  }
  let bytes = resp.bytes().await?;
  serde_json::from_slice(&bytes).map_err(|e| {
    anyhow!(
      "deserialize error: {}, body: {}",
      e,
      String::from_utf8_lossy(&bytes)
    )
  })
}

// Models
#[derive(serde::Deserialize)]
struct Token {
  token: String,
}
