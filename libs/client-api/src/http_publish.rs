use std::{
  pin::Pin,
  task::{Context, Poll},
};

use bytes::Bytes;
use client_api_entity::{PublishCollabItem, PublishInfo, UpdatePublishNamespace};
use futures::Stream;
use reqwest::{Body, Method};
use serde::Serialize;
use shared_entity::response::{AppResponse, AppResponseError};

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

  pub async fn publish_collabs<Metadata, Data>(
    &self,
    workspace_id: &str,
    items: Vec<PublishCollabItem<Metadata, Data>>,
  ) -> Result<(), AppResponseError>
  where
    Metadata: serde::Serialize + Send + 'static + Unpin,
    Data: AsRef<[u8]> + Send + 'static + Unpin,
  {
    let publish_collab_stream = PublishCollabItemStream::new(items);
    let url = format!("{}/api/workspace/{}/publish", self.base_url, workspace_id,);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .header("Content-Type", "octet-stream")
      .header("Transfer-Encoding", "chunked")
      .body(Body::wrap_stream(publish_collab_stream))
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn delete_published_collab(
    &self,
    workspace_id: &str,
    view_id: &uuid::Uuid,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/publish/{}",
      self.base_url, workspace_id, view_id
    );
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }
}

// Guest API (no login required)
impl Client {
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

  pub async fn get_published_collab<T>(
    &self,
    publish_namespace: &str,
    doc_name: &str,
  ) -> Result<T, AppResponseError>
  where
    T: serde::de::DeserializeOwned,
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
      .error_for_status()?;

    let txt = resp.text().await?;

    if let Ok(app_err) = serde_json::from_str::<AppResponseError>(&txt) {
      return Err(app_err);
    }

    let meta = serde_json::from_str::<T>(&txt)?;
    Ok(meta)
  }

  pub async fn get_published_collab_blob(
    &self,
    publish_namespace: &str,
    doc_name: &str,
  ) -> Result<Bytes, AppResponseError> {
    let url = format!(
      "{}/api/workspace/published/{}/{}/blob",
      self.base_url, publish_namespace, doc_name
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
}

pub struct PublishCollabItemStream<Metadata, Data> {
  items: Vec<PublishCollabItem<Metadata, Data>>,
  idx: usize,
}

impl<Metadata, Data> PublishCollabItemStream<Metadata, Data> {
  pub fn new(publish_collab_items: Vec<PublishCollabItem<Metadata, Data>>) -> Self {
    PublishCollabItemStream {
      items: publish_collab_items,
      idx: 0,
    }
  }
}

impl<Metadata, Data> Stream for PublishCollabItemStream<Metadata, Data>
where
  Metadata: Serialize + Send + 'static + Unpin,
  Data: AsRef<[u8]> + Send + 'static + Unpin,
{
  type Item = Result<Bytes, std::io::Error>;

  fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let mut self_mut = self.as_mut();

    if self_mut.idx >= self_mut.items.len() {
      return Poll::Ready(None);
    }

    let item = &self_mut.items[self_mut.idx];
    match serialize_metadata_data(&item.meta, item.data.as_ref()) {
      Err(e) => Poll::Ready(Some(Err(e))),
      Ok(chunk) => {
        self_mut.idx += 1;
        Poll::Ready(Some(Ok::<bytes::Bytes, std::io::Error>(chunk)))
      },
    }
  }
}

fn serialize_metadata_data<Metadata>(m: Metadata, d: &[u8]) -> Result<Bytes, std::io::Error>
where
  Metadata: Serialize,
{
  let meta = serde_json::to_vec(&m)?;

  let mut chunk = Vec::with_capacity(4 + meta.len() + d.len());
  chunk.extend_from_slice(&(meta.len() as u32).to_le_bytes()); // Encode metadata length
  chunk.extend_from_slice(&meta);
  chunk.extend_from_slice(d);

  Ok(Bytes::from(chunk))
}
