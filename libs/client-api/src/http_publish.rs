use database_entity::dto::UpdatePublishNamespace;
use reqwest::Method;
use shared_entity::response::{AppResponse, AppResponseError};

use crate::Client;

impl Client {
  pub async fn get_workspace_publish_namespace(
    &self,
    workspace_id: &str,
  ) -> Result<String, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/publish-namespace",
      self.base_url, workspace_id
    );
    let resp = self.cloud_client.get(&url).send().await?;
    AppResponse::<String>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn set_workspace_publish_namespace(
    &self,
    workspace_id: &str,
    new_namespace: String,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/publish-namespace",
      self.base_url, workspace_id
    );

    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&UpdatePublishNamespace { new_namespace })
      .send()
      .await?;

    AppResponse::<()>::from_response(resp).await?.into_error()
  }
}
