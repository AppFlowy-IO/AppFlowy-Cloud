use crate::http::log_request_id;
use crate::Client;
use database_entity::dto::{
  AFCollabMember, AFCollabMembers, AFWorkspaceInvitation, AFWorkspaceInvitationStatus,
  AFWorkspaceMember, CollabMemberIdentify, InsertCollabMemberParams, QueryCollabMembers,
  QueryWorkspaceMember, UpdateCollabMemberParams,
};
use reqwest::Method;
use shared_entity::dto::workspace_dto::{
  CreateWorkspaceMembers, WorkspaceMemberChangeset, WorkspaceMemberInvitation, WorkspaceMembers,
};
use shared_entity::response::{AppResponse, AppResponseError};
use tracing::instrument;

impl Client {
  #[instrument(level = "info", skip_all, err)]
  pub async fn leave_workspace(&self, workspace_id: &str) -> Result<(), AppResponseError> {
    let url = format!("{}/api/workspace/{}/leave", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&())
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn get_workspace_members<W: AsRef<str>>(
    &self,
    workspace_id: W,
  ) -> Result<Vec<AFWorkspaceMember>, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/member",
      self.base_url,
      workspace_id.as_ref()
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<Vec<AFWorkspaceMember>>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn invite_workspace_members(
    &self,
    workspace_id: &str,
    invitations: Vec<WorkspaceMemberInvitation>,
  ) -> Result<(), AppResponseError> {
    let url = format!("{}/api/workspace/{}/invite", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&invitations)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  #[instrument(level = "info", skip_all, err)]
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
    log_request_id(&resp);
    let res = AppResponse::<Vec<AFWorkspaceInvitation>>::from_response(resp).await?;
    res.into_data()
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
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
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn update_workspace_member<T: AsRef<str>>(
    &self,
    workspace_id: T,
    changeset: WorkspaceMemberChangeset,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/member",
      self.base_url,
      workspace_id.as_ref()
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&changeset)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn remove_workspace_members<T: AsRef<str>>(
    &self,
    workspace_id: T,
    member_emails: Vec<String>,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/member",
      self.base_url,
      workspace_id.as_ref()
    );
    let payload = WorkspaceMembers::from(member_emails);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .json(&payload)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()?;
    Ok(())
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn add_collab_member(
    &self,
    params: InsertCollabMemberParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}/member",
      self.base_url, params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn get_collab_member(
    &self,
    params: CollabMemberIdentify,
  ) -> Result<AFCollabMember, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}/member",
      self.base_url, params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<AFCollabMember>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn update_collab_member(
    &self,
    params: UpdateCollabMemberParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}/member",
      self.base_url, params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn remove_collab_member(
    &self,
    params: CollabMemberIdentify,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}/member",
      self.base_url, params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn get_collab_members(
    &self,
    params: QueryCollabMembers,
  ) -> Result<AFCollabMembers, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}/member/list",
      self.base_url, params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<AFCollabMembers>::from_response(resp)
      .await?
      .into_data()
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
    log_request_id(&resp);
    AppResponse::<AFWorkspaceMember>::from_response(resp)
      .await?
      .into_data()
  }
}
