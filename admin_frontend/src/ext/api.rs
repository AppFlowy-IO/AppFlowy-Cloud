use database_entity::dto::AFWorkspace;

use super::entities::{JsonResponse, UserUsageLimit, WorkspaceMember, WorkspaceUsage};

pub async fn get_user_workspace_count(auth_header: &str, appflowy_cloud_base_url: &str) -> u32 {
  let workspaces = get_user_workspaces(auth_header, appflowy_cloud_base_url).await;
  workspaces.len() as u32
}

async fn get_user_workspaces(auth_header: &str, appflowy_cloud_base_url: &str) -> Vec<AFWorkspace> {
  let http_client = reqwest::Client::new();
  let resp = match http_client
    .get(format!("{}/api/workspace", appflowy_cloud_base_url))
    .header("Authorization", format!("Bearer {}", auth_header))
    .send()
    .await
  {
    Ok(resp) => resp,
    Err(err) => {
      tracing::error!("Error getting user workspaces: {:?}", err);
      return vec![];
    },
  };

  let res = match resp.json::<JsonResponse<Vec<AFWorkspace>>>().await {
    Ok(res) => res,
    Err(err) => {
      tracing::error!("Error parsing user workspaces: {:?}", err);
      return vec![];
    },
  };

  res.data
}

pub async fn get_user_workspace_limit(
  auth_header: &str,
  appflowy_cloud_base_url: &str,
) -> Option<i64> {
  let http_client = reqwest::Client::new();
  let resp = match http_client
    .get(format!("{}/api/user/limit", appflowy_cloud_base_url))
    .header("Authorization", format!("Bearer {}", auth_header))
    .send()
    .await
  {
    Ok(resp) => resp,
    Err(err) => {
      tracing::warn!("unable to get user workspace limit: {:?}", err);
      return None;
    },
  };

  let res = match resp.json::<JsonResponse<UserUsageLimit>>().await {
    Ok(res) => res,
    Err(err) => {
      tracing::error!("Error parsing user workspace limit: {:?}", err);
      return None;
    },
  };
  Some(res.data.workspace_count)
}

pub async fn get_user_workspace_usages(
  auth_header: &str,
  appflowy_cloud_base_url: &str,
  appflowy_cloud_gateway_base_url: &str,
) -> Vec<WorkspaceUsage> {
  let user_workspaces = get_user_workspaces(auth_header, appflowy_cloud_base_url).await;

  let mut workspace_usages: Vec<WorkspaceUsage> = Vec::with_capacity(user_workspaces.len());
  for user_workspace in user_workspaces {
    let workspace_id = user_workspace.workspace_id.to_string();
    let members =
      get_user_workspace_members(&workspace_id, auth_header, appflowy_cloud_gateway_base_url).await;

    workspace_usages.push(WorkspaceUsage {
      name: user_workspace.workspace_name,
      member_count: members.len(),
      member_limit: 98798,                                  // todo
      total_doc_size: human_bytes::human_bytes(987654),     // todo
      total_blob_size: human_bytes::human_bytes(9876543),   // todo
      total_blob_limit: human_bytes::human_bytes(98765432), // todo
    });
  }

  workspace_usages
}

async fn get_user_workspace_members(
  workspace_id: &str,
  auth_header: &str,
  appflowy_cloud_base_url: &str,
) -> Vec<WorkspaceMember> {
  let http_client = reqwest::Client::new();
  let resp = match http_client
    .get(format!(
      "{}/api/workspace/{}/member",
      appflowy_cloud_base_url, workspace_id
    ))
    .header("Authorization", format!("Bearer {}", auth_header))
    .send()
    .await
  {
    Ok(resp) => resp,
    Err(err) => {
      tracing::error!("Error getting user workspace members: {:?}", err);
      return vec![];
    },
  };

  let res = match resp.json::<JsonResponse<Vec<WorkspaceMember>>>().await {
    Ok(res) => res,
    Err(err) => {
      tracing::error!("Error parsing user workspace limit: {:?}", err);
      return vec![];
    },
  };
  res.data
}
