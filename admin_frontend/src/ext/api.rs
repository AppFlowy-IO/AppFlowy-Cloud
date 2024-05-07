use database_entity::dto::{AFRole, AFWorkspace, AFWorkspaceInvitation};
use shared_entity::dto::{auth_dto::SignInTokenResponse, workspace_dto::WorkspaceMemberInvitation};

use super::{
  check_response,
  entities::{
    UserProfile, UserUsageLimit, WorkspaceBlobUsage, WorkspaceDocUsage, WorkspaceMember,
    WorkspaceUsageLimit, WorkspaceUsageLimits,
  },
  error::Error,
  from_json_response,
};

pub async fn get_user_owned_workspaces(
  access_token: &str,
  appflowy_cloud_base_url: &str,
) -> Result<Vec<AFWorkspace>, Error> {
  let user_profile = get_user_profile(access_token, appflowy_cloud_base_url).await?;
  let owned_workspaces = get_user_workspaces(access_token, appflowy_cloud_base_url)
    .await?
    .into_iter()
    .filter(|w| w.owner_uid == user_profile.uid)
    .collect::<Vec<_>>();
  Ok(owned_workspaces)
}

pub async fn get_user_workspaces(
  access_token: &str,
  appflowy_cloud_base_url: &str,
) -> Result<Vec<AFWorkspace>, Error> {
  let http_client = reqwest::Client::new();
  let resp = http_client
    .get(format!("{}/api/workspace", appflowy_cloud_base_url))
    .header("Authorization", format!("Bearer {}", access_token))
    .send()
    .await?;

  from_json_response(resp).await
}

pub async fn get_user_workspace_limit(
  access_token: &str,
  appflowy_cloud_base_url: &str,
) -> Result<UserUsageLimit, Error> {
  let http_client = reqwest::Client::new();
  let resp = http_client
    .get(format!("{}/api/user/limit", appflowy_cloud_base_url))
    .header("Authorization", format!("Bearer {}", access_token))
    .send()
    .await?;

  from_json_response(resp).await
}

pub async fn get_user_workspace_usages(
  access_token: &str,
  appflowy_cloud_base_url: &str,
  appflowy_cloud_gateway_base_url: &str,
) -> Result<Vec<WorkspaceUsageLimits>, Error> {
  let user_workspaces = get_user_owned_workspaces(access_token, appflowy_cloud_base_url).await?;

  let mut workspace_usages: Vec<WorkspaceUsageLimits> = Vec::with_capacity(user_workspaces.len());
  for user_workspace in user_workspaces {
    let workspace_id = user_workspace.workspace_id.to_string();
    let members =
      get_workspace_members(&workspace_id, access_token, appflowy_cloud_base_url).await?;
    let total_blob_size =
      get_user_workspace_blob_usage(&workspace_id, access_token, appflowy_cloud_base_url)
        .await
        .map(|u| human_bytes::human_bytes(u.consumed_capacity as f64))
        .unwrap_or_else(|err| {
          tracing::error!("Error getting user workspace blob usage: {:?}", err);
          "0".to_owned()
        });
    let total_doc_size = {
      get_user_workspace_doc_usage(&workspace_id, access_token, appflowy_cloud_base_url)
        .await
        .map(|u| human_bytes::human_bytes(u.total_document_size as f64))
        .unwrap_or_else(|err| {
          tracing::error!("Error getting user workspace doc usage: {:?}", err);
          "0".to_owned()
        })
    };

    let workspace_limits =
      get_user_workspace_limits(&workspace_id, access_token, appflowy_cloud_gateway_base_url).await;
    let (member_limit, total_blob_limit) = match workspace_limits {
      Ok(limit) => (
        limit.member_count.to_string(),
        human_bytes::human_bytes(limit.total_blob_size as f64),
      ),
      Err(e) => {
        tracing::warn!("Error getting user workspace limits: {:?}", e);
        ("N/A".to_string(), "N/A".to_string())
      },
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

  Ok(workspace_usages)
}

pub async fn get_workspace_members(
  workspace_id: &str,
  access_token: &str,
  appflowy_cloud_base_url: &str,
) -> Result<Vec<WorkspaceMember>, Error> {
  let http_client = reqwest::Client::new();
  let resp = http_client
    .get(format!(
      "{}/api/workspace/{}/member",
      appflowy_cloud_base_url, workspace_id
    ))
    .header("Authorization", format!("Bearer {}", access_token))
    .send()
    .await?;

  from_json_response(resp).await
}

pub async fn get_pending_workspace_invitations(
  access_token: &str,
  appflowy_cloud_base_url: &str,
) -> Result<Vec<AFWorkspaceInvitation>, Error> {
  let http_client = reqwest::Client::new();
  let resp = http_client
    .get(format!(
      "{}/api/workspace/invite?status=Pending",
      appflowy_cloud_base_url
    ))
    .header("Authorization", format!("Bearer {}", access_token))
    .send()
    .await?;

  from_json_response(resp).await
}

pub async fn get_accepted_workspace_invitations(
  access_token: &str,
  appflowy_cloud_base_url: &str,
) -> Result<Vec<AFWorkspaceInvitation>, Error> {
  let http_client = reqwest::Client::new();
  let resp = http_client
    .get(format!(
      "{}/api/workspace/invite?status=Accepted",
      appflowy_cloud_base_url
    ))
    .header("Authorization", format!("Bearer {}", access_token))
    .send()
    .await?;

  from_json_response(resp).await
}

async fn get_user_workspace_limits(
  workspace_id: &str,
  access_token: &str,
  appflowy_cloud_gateway_base_url: &str,
) -> Result<WorkspaceUsageLimit, Error> {
  let http_client = reqwest::Client::new();
  let resp = http_client
    .get(format!(
      "{}/api/workspace/{}/limit",
      appflowy_cloud_gateway_base_url, workspace_id
    ))
    .header("Authorization", format!("Bearer {}", access_token))
    .send()
    .await?;

  from_json_response(resp).await
}

async fn get_user_workspace_blob_usage(
  workspace_id: &str,
  access_token: &str,
  appflowy_cloud_gateway_base_url: &str,
) -> Result<WorkspaceBlobUsage, Error> {
  let http_client = reqwest::Client::new();
  let resp = http_client
    .get(format!(
      "{}/api/file_storage/{}/usage",
      appflowy_cloud_gateway_base_url, workspace_id
    ))
    .header("Authorization", format!("Bearer {}", access_token))
    .send()
    .await?;

  from_json_response(resp).await
}

async fn get_user_workspace_doc_usage(
  workspace_id: &str,
  access_token: &str,
  appflowy_cloud_base_url: &str,
) -> Result<WorkspaceDocUsage, Error> {
  let http_client = reqwest::Client::new();
  let url = format!(
    "{}/api/workspace/{}/usage",
    appflowy_cloud_base_url, workspace_id
  );
  let resp = http_client
    .get(url)
    .header("Authorization", format!("Bearer {}", access_token))
    .send()
    .await?;

  from_json_response(resp).await
}

pub async fn get_user_profile(
  access_token: &str,
  appflowy_cloud_base_url: &str,
) -> Result<UserProfile, Error> {
  let http_client = reqwest::Client::new();
  let url = format!("{}/api/user/profile", appflowy_cloud_base_url);
  let resp = http_client
    .get(url)
    .header("Authorization", format!("Bearer {}", access_token))
    .send()
    .await?;
  from_json_response(resp).await
}

pub async fn invite_user_to_workspace(
  access_token: &str,
  workspace_id: &str,
  user_email: &str,
  appflowy_cloud_base_url: &str,
) -> Result<(), Error> {
  let invi = vec![WorkspaceMemberInvitation {
    email: user_email.to_string(),
    role: AFRole::Member,
  }];

  let http_client = reqwest::Client::new();
  let url = format!(
    "{}/api/workspace/{}/invite",
    appflowy_cloud_base_url, workspace_id
  );
  let resp = http_client
    .post(url)
    .header("Authorization", format!("Bearer {}", access_token))
    .json(&invi)
    .send()
    .await?;

  check_response(resp).await
}

pub async fn leave_workspace(
  access_token: &str,
  workspace_id: &str,
  appflowy_cloud_base_url: &str,
) -> Result<(), Error> {
  let http_client = reqwest::Client::new();
  let url = format!(
    "{}/api/workspace/{}/leave",
    appflowy_cloud_base_url, workspace_id
  );
  let resp = http_client
    .post(url)
    .header("Authorization", format!("Bearer {}", access_token))
    .json(&())
    .send()
    .await?;

  check_response(resp).await
}

pub async fn accept_workspace_invitation(
  access_token: &str,
  invite_id: &str,
  appflowy_cloud_base_url: &str,
) -> Result<(), Error> {
  let http_client = reqwest::Client::new();
  let url = format!(
    "{}/api/workspace/accept-invite/{}",
    appflowy_cloud_base_url, invite_id
  );
  let resp = http_client
    .post(url)
    .header("Authorization", format!("Bearer {}", access_token))
    .json(&())
    .send()
    .await?;

  check_response(resp).await
}

pub async fn verify_token_cloud(
  access_token: &str,
  appflowy_cloud_base_url: &str,
) -> Result<(), Error> {
  let http_client = reqwest::Client::new();
  let url = format!(
    "{}/api/user/verify/{}",
    appflowy_cloud_base_url, access_token
  );
  let resp = http_client
    .get(url)
    .header("Authorization", format!("Bearer {}", access_token))
    .send()
    .await?;
  let _: SignInTokenResponse = from_json_response(resp).await?;
  Ok(())
}
