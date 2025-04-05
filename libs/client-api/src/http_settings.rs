use reqwest::Method;
use tracing::{instrument, trace};

use client_api_entity::AFWorkspaceSettings;
use shared_entity::response::AppResponseError;

use crate::entity::AFWorkspaceSettingsChange;
use crate::{process_response_data, Client};

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
    process_response_data::<AFWorkspaceSettings>(resp).await
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
    process_response_data::<AFWorkspaceSettings>(resp).await
  }
}
