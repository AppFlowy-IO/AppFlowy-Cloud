use anyhow::anyhow;
use anyhow::Error;

pub async fn check_response(resp: reqwest::Response) -> Result<(), Error> {
  let status_code = resp.status();
  if !status_code.is_success() {
    let body = resp.text().await?;
    anyhow::bail!("got error code: {}, body: {}", status_code, body)
  }
  resp.bytes().await?;
  Ok(())
}

pub async fn from_response<T>(resp: reqwest::Response) -> Result<T, Error>
where
  T: serde::de::DeserializeOwned,
{
  let status_code = resp.status();
  if !status_code.is_success() {
    let body = resp.text().await?;
    anyhow::bail!("got error code: {}, body: {}", status_code, body)
  }
  from_body(resp).await
}

pub async fn from_body<T>(resp: reqwest::Response) -> Result<T, Error>
where
  T: serde::de::DeserializeOwned,
{
  let status_code = resp.status();
  let bytes = resp.bytes().await?;
  serde_json::from_slice(&bytes).map_err(|e| {
    anyhow!(
      "deserialize error: {}, status: {}, body: {}",
      status_code,
      e,
      String::from_utf8_lossy(&bytes)
    )
  })
}
