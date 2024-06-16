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
    new_namespace: &str,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/publish-namespace",
      self.base_url, workspace_id
    );

    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&UpdatePublishNamespace {
        new_namespace: new_namespace.to_string(),
      })
      .send()
      .await?;

    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn publish_collab<T>(
    &self,
    workspace_id: &str,
    doc_name: &str,
    metadata: T,
  ) -> Result<(), AppResponseError>
  where
    T: serde::Serialize,
  {
    let url = format!(
      "{}/api/workspace/{}/publish/{}",
      self.base_url, workspace_id, doc_name
    );

    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&metadata)
      .send()
      .await?;

    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn get_published_collab<T>(
    &self,
    workspace_id: &str,
    doc_name: &str,
  ) -> Result<T, AppResponseError>
  where
    T: serde::de::DeserializeOwned + 'static,
  {
    let url = format!(
      "{}/api/workspace/{}/publish/{}",
      self.base_url, workspace_id, doc_name
    );

    let resp = self
      .cloud_client
      .get(&url)
      .send()
      .await?
      .error_for_status()?
      .json::<T>()
      .await?;
    Ok(resp)
  }

  pub async fn get_published_collab_using_publish_namespace<T>(
    &self,
    publish_namespace: &str,
    doc_name: &str,
  ) -> Result<T, AppResponseError>
  where
    T: serde::de::DeserializeOwned + 'static,
  {
    let url = format!(
      "{}/api/workspace/published/{}/{}",
      self.base_url, publish_namespace, doc_name
    );

    let resp = self
      .cloud_client
      .get(&url)
      .send()
      .await?
      .error_for_status()?
      .json::<T>()
      .await?;
    Ok(resp)
  }
}
