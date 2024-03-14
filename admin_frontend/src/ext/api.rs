use database_entity::dto::AFWorkspace;

use super::entities::{
  JsonResponse, UserUsageLimit, WorkspaceBlobUsage, WorkspaceDocUsage, WorkspaceMember,
  WorkspaceUsageLimit, WorkspaceUsageLimits,
};

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
) -> Vec<WorkspaceUsageLimits> {
  let user_workspaces = get_user_workspaces(auth_header, appflowy_cloud_base_url).await;

  let mut workspace_usages: Vec<WorkspaceUsageLimits> = Vec::with_capacity(user_workspaces.len());
  for user_workspace in user_workspaces {
    let workspace_id = user_workspace.workspace_id.to_string();
    let members =
      get_user_workspace_members(&workspace_id, auth_header, appflowy_cloud_base_url).await;
    let workspace_limits =
      get_user_workspace_limits(&workspace_id, auth_header, appflowy_cloud_gateway_base_url).await;
    let total_blob_size =
      get_user_workspace_blob_usage(&workspace_id, auth_header, appflowy_cloud_base_url)
        .await
        .map(|u| human_bytes::human_bytes(u.consumed_capacity as f64))
        .unwrap_or_else(|err| {
          tracing::error!("Error getting user workspace blob usage: {:?}", err);
          "0".to_owned()
        });

    let total_doc_size = {
      get_user_workspace_doc_usage(&workspace_id, auth_header, appflowy_cloud_base_url)
        .await
        .map(|u| human_bytes::human_bytes(u.total_document_size as f64))
        .unwrap_or_else(|err| {
          tracing::error!("Error getting user workspace doc usage: {:?}", err);
          "0".to_owned()
        })
    };

    let (member_limit, total_blob_limit) = match workspace_limits {
      Some(limit) => (
        limit.member_count.to_string(),
        human_bytes::human_bytes(limit.total_blob_size as f64),
      ),
      None => ("N/A".to_string(), "N/A".to_string()),
    };

    workspace_usages.push(WorkspaceUsageLimits {
      name: user_workspace.workspace_name,
      member_count: members.len(),
      member_limit,
      total_doc_size,
      total_blob_size,
      total_blob_limit,
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

async fn get_user_workspace_limits(
  workspace_id: &str,
  auth_header: &str,
  appflowy_cloud_gateway_base_url: &str,
) -> Option<WorkspaceUsageLimit> {
  let http_client = reqwest::Client::new();
  let resp = match http_client
    .get(format!(
      "{}/api/workspace/{}/limit",
      appflowy_cloud_gateway_base_url, workspace_id
    ))
    .header("Authorization", format!("Bearer {}", auth_header))
    .send()
    .await
  {
    Ok(resp) => resp,
    Err(err) => {
      tracing::error!("Error getting user workspace members: {:?}", err);
      return None;
    },
  };

  let res = match resp.json::<JsonResponse<WorkspaceUsageLimit>>().await {
    Ok(res) => res,
    Err(err) => {
      tracing::error!("Error parsing user workspace limit: {:?}", err);
      return None;
    },
  };
  Some(res.data)
}

async fn get_user_workspace_blob_usage(
  workspace_id: &str,
  auth_header: &str,
  appflowy_cloud_gateway_base_url: &str,
) -> Result<WorkspaceBlobUsage, reqwest::Error> {
  let http_client = reqwest::Client::new();
  let resp = http_client
    .get(format!(
      "{}/api/file_storage/{}/usage",
      appflowy_cloud_gateway_base_url, workspace_id
    ))
    .header("Authorization", format!("Bearer {}", auth_header))
    .send()
    .await?;

  let res = resp.json::<JsonResponse<WorkspaceBlobUsage>>().await?;
  Ok(res.data)
}

async fn get_user_workspace_doc_usage(
  workspace_id: &str,
  auth_header: &str,
  appflowy_cloud_base_url: &str,
) -> Result<WorkspaceDocUsage, reqwest::Error> {
  let http_client = reqwest::Client::new();
  let url = format!(
    "{}/api/workspace/{}/usage",
    appflowy_cloud_base_url, workspace_id
  );
  let resp = http_client
    .get(url)
    .header("Authorization", format!("Bearer {}", auth_header))
    .send()
    .await?;

  let res = resp.json::<JsonResponse<WorkspaceDocUsage>>().await?;
  Ok(res.data)
}
