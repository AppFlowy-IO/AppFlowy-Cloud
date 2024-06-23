use reqwest::Method;
use tracing::{instrument, trace};

use client_api_entity::AFWorkspaceSettings;
use shared_entity::response::{AppResponse, AppResponseError};

use crate::entity::AFWorkspaceSettingsChange;
use crate::http::log_request_id;
use crate::Client;

impl Client {
  #[instrument(level = "info", skip_all, err)]
  pub async fn get_workspace_settings<T: AsRef<str>>(
    &self,
    workspace_id: T,
  ) -> Result<AFWorkspaceSettings, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/settings",
      self.base_url,
      workspace_id.as_ref()
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    let resp = AppResponse::<AFWorkspaceSettings>::from_response(resp).await?;
    resp.into_data()
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn update_workspace_settings<T: AsRef<str>>(
    &self,
    workspace_id: T,
    changes: &AFWorkspaceSettingsChange,
  ) -> Result<AFWorkspaceSettings, AppResponseError> {
    trace!("workspace settings: {:?}", changes);
    let url = format!(
      "{}/api/workspace/{}/settings",
      self.base_url,
      workspace_id.as_ref()
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&changes)
      .send()
      .await?;
    log_request_id(&resp);
    let resp = AppResponse::<AFWorkspaceSettings>::from_response(resp).await?;
    resp.into_data()
  }
}
