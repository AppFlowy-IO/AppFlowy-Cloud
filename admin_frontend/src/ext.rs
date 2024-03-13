use database_entity::dto::AFWorkspace;
use serde::Deserialize;

pub async fn get_user_workspace_count(
  auth_header: &str,
  appflowy_cloud_base_url: &str,
) -> Result<u32, reqwest::Error> {
  let http_client = reqwest::Client::new();
  let resp = http_client
    .get(format!("{}/api/workspace", appflowy_cloud_base_url))
    .header("Authorization", format!("Bearer {}", auth_header))
    .send()
    .await?;

  let res = resp.json::<JsonResponse<Vec<AFWorkspace>>>().await?;
  Ok(res.data.len() as u32)
}

pub async fn get_user_workspace_limit(
  auth_header: &str,
  appflowy_cloud_base_url: &str,
) -> Result<u32, reqwest::Error> {
  let http_client = reqwest::Client::new();
  let resp = http_client
    .get(format!("{}/api/user/limit", appflowy_cloud_base_url))
    .header("Authorization", format!("Bearer {}", auth_header))
    .send()
    .await?;

  let res = resp.json::<JsonResponse<UserUsageLimit>>().await?;
  let b = res.data;
  let c = b.workspace_count.unwrap_or({
    tracing::warn!("workspace_count is None, returning 0");
    0
  });
  Ok(c as u32)
}

#[derive(Debug, Deserialize)]
pub struct JsonResponse<T> {
  pub code: u16,
  pub data: T,
}

#[derive(Deserialize)]
pub struct UserUsageLimit {
  pub workspace_count: Option<i64>,
}
