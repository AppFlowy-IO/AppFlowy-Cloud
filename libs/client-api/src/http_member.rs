use crate::{Client, process_response_data, process_response_error};
use client_api_entity::{
  AFWorkspaceInvitation, AFWorkspaceInvitationStatus, AFWorkspaceMember, QueryWorkspaceMember,
};
use reqwest::Method;
use shared_entity::dto::workspace_dto::{
  CreateWorkspaceMembers, WorkspaceMemberChangeset, WorkspaceMemberInvitation, WorkspaceMembers,
};
use shared_entity::response::AppResponseError;
use tracing::instrument;
use uuid::Uuid;

impl Client {
  #[instrument(level = "info", skip_all, err)]
  pub async fn leave_workspace(&self, workspace_id: &Uuid) -> Result<(), AppResponseError> {
    let url = format!("{}/api/workspace/{}/leave", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&())
      .send()
      .await?;
    process_response_error(resp).await
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn get_workspace_members(
    &self,
    workspace_id: &Uuid,
  ) -> Result<Vec<AFWorkspaceMember>, AppResponseError> {
    let url = format!("{}/api/workspace/{}/member", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<Vec<AFWorkspaceMember>>(resp).await
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn invite_workspace_members(
    &self,
    workspace_id: &Uuid,
    invitations: Vec<WorkspaceMemberInvitation>,
  ) -> Result<(), AppResponseError> {
    let url = format!("{}/api/workspace/{}/invite", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&invitations)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn list_workspace_invitations(
    &self,
    status: Option<AFWorkspaceInvitationStatus>,
  ) -> Result<Vec<AFWorkspaceInvitation>, AppResponseError> {
    let url = format!("{}/api/workspace/invite", self.base_url);
    let mut builder = self.http_client_with_auth(Method::GET, &url).await?;
    if let Some(status) = status {
      builder = builder.query(&[("status", status)])
    }
    let resp = builder.send().await?;
    process_response_data::<Vec<AFWorkspaceInvitation>>(resp).await
  }

  pub async fn get_workspace_invitation(
    &self,
    invite_uuid: &str,
  ) -> Result<AFWorkspaceInvitation, AppResponseError> {
    let url = format!("{}/api/workspace/invite/{}", self.base_url, invite_uuid);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<AFWorkspaceInvitation>(resp).await
  }

  pub async fn accept_workspace_invitation(
    &self,
    invitation_id: &str,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/accept-invite/{}",
      self.base_url, invitation_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&())
      .send()
      .await?;
    process_response_error(resp).await
  }

  #[deprecated(note = "use invite_workspace_members instead")]
  #[instrument(level = "info", skip_all, err)]
  pub async fn add_workspace_members<T: Into<CreateWorkspaceMembers>, W: AsRef<str>>(
    &self,
    workspace_id: W,
    members: T,
  ) -> Result<(), AppResponseError> {
    let members = members.into();
    let url = format!(
      "{}/api/workspace/{}/member",
      self.base_url,
      workspace_id.as_ref()
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&members)
      .send()
      .await?;
    process_response_error(resp).await
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn update_workspace_member(
    &self,
    workspace_id: &Uuid,
    changeset: WorkspaceMemberChangeset,
  ) -> Result<(), AppResponseError> {
    let url = format!("{}/api/workspace/{}/member", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&changeset)
      .send()
      .await?;
    process_response_error(resp).await
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn remove_workspace_members(
    &self,
    workspace_id: &Uuid,
    member_emails: Vec<String>,
  ) -> Result<(), AppResponseError> {
    let url = format!("{}/api/workspace/{}/member", self.base_url, workspace_id);
    let payload = WorkspaceMembers::from(member_emails);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .json(&payload)
      .send()
      .await?;
    process_response_error(resp).await
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn get_workspace_member(
    &self,
    params: QueryWorkspaceMember,
  ) -> Result<AFWorkspaceMember, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/member/user/{}",
      self.base_url, params.workspace_id, params.uid,
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<AFWorkspaceMember>(resp).await
  }
}
