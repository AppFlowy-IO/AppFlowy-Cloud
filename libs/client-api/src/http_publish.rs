use bytes::Bytes;
use client_api_entity::{workspace_dto::PublishedDuplicate, PublishInfo, UpdatePublishNamespace};
use client_api_entity::{
  CreateGlobalCommentParams, CreateReactionParams, DeleteGlobalCommentParams, DeleteReactionParams,
  GetReactionQueryParams, GlobalComments, Reactions,
};
use reqwest::Method;
use shared_entity::response::{AppResponse, AppResponseError};
use tracing::instrument;

use crate::Client;

// Publisher API
impl Client {
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

  pub async fn get_workspace_publish_namespace(
    &self,
    workspace_id: &str,
  ) -> Result<String, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/publish-namespace",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<String>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn unpublish_collabs(
    &self,
    workspace_id: &str,
    view_ids: &[uuid::Uuid],
  ) -> Result<(), AppResponseError> {
    let url = format!("{}/api/workspace/{}/publish", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .json(view_ids)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn create_comment_on_published_view(
    &self,
    view_id: &uuid::Uuid,
    comment_content: &str,
    reply_comment_id: &Option<uuid::Uuid>,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/published-info/{}/comment",
      self.base_url, view_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&CreateGlobalCommentParams {
        content: comment_content.to_string(),
        reply_comment_id: *reply_comment_id,
      })
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn delete_comment_on_published_view(
    &self,
    view_id: &uuid::Uuid,
    comment_id: &uuid::Uuid,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/published-info/{}/comment",
      self.base_url, view_id
    );
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .json(&DeleteGlobalCommentParams {
        comment_id: *comment_id,
      })
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn create_reaction_on_comment(
    &self,
    reaction_type: &str,
    view_id: &uuid::Uuid,
    comment_id: &uuid::Uuid,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/published-info/{}/reaction",
      self.base_url, view_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&CreateReactionParams {
        reaction_type: reaction_type.to_string(),
        comment_id: *comment_id,
      })
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn delete_reaction_on_comment(
    &self,
    reaction_type: &str,
    view_id: &uuid::Uuid,
    comment_id: &uuid::Uuid,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/published-info/{}/reaction",
      self.base_url, view_id
    );
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .json(&DeleteReactionParams {
        reaction_type: reaction_type.to_string(),
        comment_id: *comment_id,
      })
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }
}

// Optional login
impl Client {
  pub async fn get_published_view_comments(
    &self,
    view_id: &uuid::Uuid,
  ) -> Result<GlobalComments, AppResponseError> {
    let url = format!(
      "{}/api/workspace/published-info/{}/comment",
      self.base_url, view_id
    );
    let client = if let Ok(client) = self.http_client_with_auth(Method::GET, &url).await {
      client
    } else {
      self.http_client_without_auth(Method::GET, &url).await?
    };

    let resp = client.send().await?;
    AppResponse::<GlobalComments>::from_response(resp)
      .await?
      .into_data()
  }
}

// Guest API (no login required)
impl Client {
  #[instrument(level = "debug", skip_all)]
  pub async fn get_published_collab_info(
    &self,
    view_id: &uuid::Uuid,
  ) -> Result<PublishInfo, AppResponseError> {
    let url = format!("{}/api/workspace/published-info/{}", self.base_url, view_id,);

    let resp = self.cloud_client.get(&url).send().await?;
    AppResponse::<PublishInfo>::from_response(resp)
      .await?
      .into_data()
  }

  #[instrument(level = "debug", skip_all)]
  pub async fn get_published_collab<T>(
    &self,
    publish_namespace: &str,
    publish_name: &str,
  ) -> Result<T, AppResponseError>
  where
    T: serde::de::DeserializeOwned,
  {
    tracing::debug!(
      "get_published_collab: {} {}",
      publish_namespace,
      publish_name
    );
    let url = format!(
      "{}/api/workspace/published/{}/{}",
      self.base_url, publish_namespace, publish_name
    );

    let resp = self
      .cloud_client
      .get(&url)
      .send()
      .await?
      .error_for_status()?;

    let txt = resp.text().await?;

    if let Ok(app_err) = serde_json::from_str::<AppResponseError>(&txt) {
      return Err(app_err);
    }

    let meta = serde_json::from_str::<T>(&txt)?;

    Ok(meta)
  }

  #[instrument(level = "debug", skip_all)]
  pub async fn get_published_collab_blob(
    &self,
    publish_namespace: &str,
    publish_name: &str,
  ) -> Result<Bytes, AppResponseError> {
    tracing::debug!(
      "get_published_collab_blob: {} {}",
      publish_namespace,
      publish_name
    );
    let url = format!(
      "{}/api/workspace/published/{}/{}/blob",
      self.base_url, publish_namespace, publish_name
    );
    let bytes = self
      .cloud_client
      .get(&url)
      .send()
      .await?
      .error_for_status()?
      .bytes()
      .await?;

    if let Ok(app_err) = serde_json::from_slice::<AppResponseError>(&bytes) {
      return Err(app_err);
    }

    Ok(bytes)
  }

  pub async fn duplicate_published_to_workspace(
    &self,
    workspace_id: &str,
    publish_duplicate: &PublishedDuplicate,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/published-duplicate",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(publish_duplicate)
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn get_published_view_reactions(
    &self,
    view_id: &uuid::Uuid,
    comment_id: &Option<uuid::Uuid>,
  ) -> Result<Reactions, AppResponseError> {
    let url = format!(
      "{}/api/workspace/published-info/{}/reaction",
      self.base_url, view_id
    );
    let resp = self
      .cloud_client
      .get(url)
      .query(&GetReactionQueryParams {
        comment_id: *comment_id,
      })
      .send()
      .await?;
    AppResponse::<Reactions>::from_response(resp)
      .await?
      .into_data()
  }
}
