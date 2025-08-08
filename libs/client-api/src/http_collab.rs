use crate::entity::CollabType;
use crate::{
  blocking_brotli_compress, brotli_compress, process_response_data, process_response_error, Client,
};
use anyhow::anyhow;
use app_error::AppError;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use client_api_entity::workspace_dto::{
  AFDatabase, AFDatabaseField, AFDatabaseRow, AFDatabaseRowDetail, AFInsertDatabaseField,
  AddDatatabaseRow, DatabaseRowUpdatedItem, ListDatabaseRowDetailParam,
  ListDatabaseRowUpdatedParam, UpsertDatatabaseRow,
};
use client_api_entity::{
  AFCollabEmbedInfo, AFDatabaseRowDocumentCollabExistenceInfo, BatchQueryCollabParams,
  BatchQueryCollabResult, CollabParams, CreateCollabData, CreateCollabParams, DeleteCollabParams,
  PublishCollabItem, QueryCollab, QueryCollabParams, RepeatedAFCollabEmbedInfo,
  UpdateCollabWebParams,
};
use collab_rt_entity::collab_proto::{CollabDocStateParams, PayloadCompressionType};
use collab_rt_entity::HttpRealtimeMessage;
use futures::Stream;
use futures_util::stream;
use prost::Message;
use rayon::prelude::*;
use reqwest::{Body, Method};
use serde::Serialize;
use shared_entity::dto::workspace_dto::{CollabResponse, CollabTypeParam, EmbeddedCollabQuery};
use shared_entity::response::AppResponseError;
use std::collections::HashMap;
use std::future::Future;
use std::io::Cursor;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio_retry::strategy::ExponentialBackoff;
use tokio_retry::{Action, Condition, RetryIf};
use tracing::{event, instrument};
use uuid::Uuid;

impl Client {
  #[instrument(level = "info", skip_all, err)]
  pub async fn create_collab(&self, params: CreateCollabParams) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}",
      self.base_url, params.workspace_id, &params.object_id
    );
    let bytes = params
      .to_bytes()
      .map_err(|err| AppError::Internal(err.into()))?;

    let compress_bytes = blocking_brotli_compress(
      bytes,
      self.config.compression_quality,
      self.config.compression_buffer_size,
    )
    .await?;

    #[allow(unused_mut)]
    let mut builder = self
      .http_client_with_auth_compress(Method::POST, &url)
      .await?;

    #[cfg(not(target_arch = "wasm32"))]
    {
      builder = builder.timeout(std::time::Duration::from_secs(60));
    }

    let resp = builder.body(compress_bytes).send().await?;
    process_response_error(resp).await
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn update_collab(&self, params: CreateCollabParams) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}",
      self.base_url, &params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn update_web_collab(
    &self,
    workspace_id: &Uuid,
    object_id: &Uuid,
    params: UpdateCollabWebParams,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/v1/{}/collab/{}/web-update",
      self.base_url, workspace_id, object_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  // The browser will call this API to get the collab list, because the URL length limit and browser can't send the body in GET request
  #[instrument(level = "info", skip_all, err)]
  pub async fn batch_post_collab(
    &self,
    workspace_id: &Uuid,
    params: Vec<QueryCollab>,
  ) -> Result<BatchQueryCollabResult, AppResponseError> {
    self
      .send_batch_collab_request(Method::POST, workspace_id, params)
      .await
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn batch_get_collab(
    &self,
    workspace_id: &Uuid,
    params: Vec<QueryCollab>,
  ) -> Result<BatchQueryCollabResult, AppResponseError> {
    self
      .send_batch_collab_request(Method::GET, workspace_id, params)
      .await
  }

  async fn send_batch_collab_request(
    &self,
    method: Method,
    workspace_id: &Uuid,
    params: Vec<QueryCollab>,
  ) -> Result<BatchQueryCollabResult, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab_list",
      self.base_url, workspace_id
    );
    let params = BatchQueryCollabParams(params);
    let resp = self
      .http_client_with_auth(method, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_data::<BatchQueryCollabResult>(resp).await
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn delete_collab(&self, params: DeleteCollabParams) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/collab/{}",
      self.base_url, &params.workspace_id, &params.object_id
    );
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_error(resp).await
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn list_databases(
    &self,
    workspace_id: &Uuid,
  ) -> Result<Vec<AFDatabase>, AppResponseError> {
    let url = format!("{}/api/workspace/{}/database", self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<Vec<AFDatabase>>(resp).await
  }

  pub async fn list_database_row_ids(
    &self,
    workspace_id: &Uuid,
    database_id: &str,
  ) -> Result<Vec<AFDatabaseRow>, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/database/{}/row",
      self.base_url, workspace_id, database_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<Vec<AFDatabaseRow>>(resp).await
  }

  pub async fn get_database_fields(
    &self,
    workspace_id: &Uuid,
    database_id: &str,
  ) -> Result<Vec<AFDatabaseField>, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/database/{}/fields",
      self.base_url, workspace_id, database_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<Vec<AFDatabaseField>>(resp).await
  }

  // Adds a database field to the specified database.
  // Returns the field id of the newly created field.
  pub async fn add_database_field(
    &self,
    workspace_id: &Uuid,
    database_id: &str,
    insert_field: &AFInsertDatabaseField,
  ) -> Result<String, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/database/{}/fields",
      self.base_url, workspace_id, database_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(insert_field)
      .send()
      .await?;
    process_response_data::<String>(resp).await
  }

  pub async fn list_database_row_ids_updated(
    &self,
    workspace_id: &Uuid,
    database_id: &str,
    after: Option<DateTime<Utc>>,
  ) -> Result<Vec<DatabaseRowUpdatedItem>, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/database/{}/row/updated",
      self.base_url, workspace_id, database_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .query(&ListDatabaseRowUpdatedParam { after })
      .send()
      .await?;
    process_response_data::<Vec<DatabaseRowUpdatedItem>>(resp).await
  }

  pub async fn list_database_row_details(
    &self,
    workspace_id: &Uuid,
    database_id: &str,
    row_ids: &[&str],
    with_doc: bool,
  ) -> Result<Vec<AFDatabaseRowDetail>, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/database/{}/row/detail",
      self.base_url, workspace_id, database_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .query(&ListDatabaseRowDetailParam::new(row_ids, with_doc))
      .send()
      .await?;
    process_response_data::<Vec<AFDatabaseRowDetail>>(resp).await
  }

  /// Example payload:
  /// {
  ///   "Name": "some_data",        # using column name
  ///   "_pIkG": "some other data"  # using field_id (can be obtained from [get_database_fields])
  /// }
  /// Upon success, returns the row id for the newly created row.
  pub async fn add_database_item(
    &self,
    workspace_id: &Uuid,
    database_id: &str,
    cells_by_id: HashMap<String, serde_json::Value>,
    row_doc_content: Option<String>,
  ) -> Result<String, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/database/{}/row",
      self.base_url, workspace_id, database_id
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&AddDatatabaseRow {
        cells: cells_by_id,
        document: row_doc_content,
      })
      .send()
      .await?;
    process_response_data::<String>(resp).await
  }

  /// Like [add_database_item], but use a [pre_hash] as identifier of the row
  /// Given the same `pre_hash` value will result in the same row
  /// Creates the row if now exists, else row will be modified
  pub async fn upsert_database_item(
    &self,
    workspace_id: &Uuid,
    database_id: &str,
    pre_hash: String,
    cells_by_id: HashMap<String, serde_json::Value>,
    row_doc_content: Option<String>,
  ) -> Result<String, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/database/{}/row",
      self.base_url, workspace_id, database_id
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&UpsertDatatabaseRow {
        pre_hash,
        cells: cells_by_id,
        document: row_doc_content,
      })
      .send()
      .await?;
    process_response_data::<String>(resp).await
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn post_realtime_msg(
    &self,
    device_id: &str,
    msg: client_websocket::Message,
  ) -> Result<(), AppResponseError> {
    let device_id = device_id.to_string();
    let payload =
      blocking_brotli_compress(msg.into_data(), 6, self.config.compression_buffer_size).await?;

    let msg = HttpRealtimeMessage { device_id, payload }.encode_to_vec();
    let body = Body::wrap_stream(stream::iter(vec![Ok::<_, reqwest::Error>(msg)]));
    let url = format!("{}/api/realtime/post/stream", self.base_url);
    let resp = self
      .http_client_with_auth_compress(Method::POST, &url)
      .await?
      .body(body)
      .send()
      .await?;
    process_response_error(resp).await
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn create_collab_list(
    &self,
    workspace_id: &Uuid,
    params_list: Vec<CollabParams>,
  ) -> Result<(), AppResponseError> {
    let url = self.batch_create_collab_url(workspace_id);

    let compression_tasks = params_list
      .into_par_iter()
      .filter_map(|params| {
        let data = CreateCollabData::from(params).to_bytes().ok()?;
        brotli_compress(
          data,
          self.config.compression_quality,
          self.config.compression_buffer_size,
        )
        .ok()
      })
      .collect::<Vec<_>>();

    let mut framed_data = Vec::new();
    let mut size_count = 0;
    for compressed in compression_tasks {
      // The length of a u32 in bytes is 4. The server uses a u32 to read the size of each data frame,
      // hence the frame size header is always 4 bytes. It's crucial not to alter this size value,
      // as the server's logic for frame size reading is based on this fixed 4-byte length.
      // note:
      // the size of a u32 is a constant 4 bytes across all platforms that Rust supports.
      let size = compressed.len() as u32;
      framed_data.extend_from_slice(&size.to_be_bytes());
      framed_data.extend_from_slice(&compressed);
      size_count += size;
    }
    event!(
      tracing::Level::INFO,
      "create batch collab with size: {}",
      size_count
    );
    let body = Body::wrap_stream(stream::once(async { Ok::<_, AppError>(framed_data) }));
    let resp = self
      .http_client_with_auth_compress(Method::POST, &url)
      .await?
      .timeout(Duration::from_secs(60))
      .body(body)
      .send()
      .await?;

    process_response_error(resp).await
  }

  #[instrument(level = "debug", skip_all)]
  pub async fn get_collab(
    &self,
    params: QueryCollabParams,
  ) -> Result<CollabResponse, AppResponseError> {
    // 2 seconds, 4 seconds, 8 seconds
    let retry_strategy = ExponentialBackoff::from_millis(2).factor(1000).take(3);
    let action = GetCollabAction::new(self.clone(), params);
    RetryIf::spawn(retry_strategy, action, RetryGetCollabCondition).await
  }

  pub async fn publish_collabs<Metadata, Data>(
    &self,
    workspace_id: &Uuid,
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
      .body(Body::wrap_stream(publish_collab_stream))
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn check_if_row_document_collab_exists(
    &self,
    workspace_id: &Uuid,
    object_id: &Uuid,
  ) -> Result<bool, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{workspace_id}/collab/{object_id}/row-document-collab-exists",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    let info = process_response_data::<AFDatabaseRowDocumentCollabExistenceInfo>(resp).await?;
    Ok(info.exists)
  }

  pub async fn get_collab_embed_info(
    &self,
    workspace_id: &Uuid,
    object_id: &Uuid,
  ) -> Result<AFCollabEmbedInfo, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{workspace_id}/collab/{object_id}/embed-info",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .header("Content-Type", "application/json")
      .send()
      .await?;
    process_response_data::<AFCollabEmbedInfo>(resp).await
  }

  pub async fn batch_get_collab_embed_info(
    &self,
    workspace_id: &Uuid,
    params: Vec<EmbeddedCollabQuery>,
  ) -> Result<Vec<AFCollabEmbedInfo>, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{workspace_id}/collab/embed-info/list",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&params)
      .send()
      .await?;
    process_response_data::<RepeatedAFCollabEmbedInfo>(resp)
      .await
      .map(|data| data.0)
  }

  pub async fn force_generate_collab_embeddings(
    &self,
    workspace_id: &Uuid,
    object_id: &Uuid,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{workspace_id}/collab/{object_id}/generate-embedding",
      self.base_url
    );
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn collab_full_sync(
    &self,
    workspace_id: &Uuid,
    object_id: &Uuid,
    collab_type: CollabType,
    doc_state: Vec<u8>,
    state_vector: Vec<u8>,
  ) -> Result<Vec<u8>, AppResponseError> {
    let url = format!(
      "{}/api/workspace/v1/{workspace_id}/collab/{object_id}/full-sync",
      self.base_url
    );

    // 3 is default level
    let doc_state = zstd::encode_all(Cursor::new(doc_state), 3)
      .map_err(|err| AppError::InvalidRequest(format!("Failed to compress text: {}", err)))?;

    let sv = zstd::encode_all(Cursor::new(state_vector), 3)
      .map_err(|err| AppError::InvalidRequest(format!("Failed to compress text: {}", err)))?;

    let params = CollabDocStateParams {
      object_id: object_id.to_string(),
      collab_type: collab_type.value(),
      compression: PayloadCompressionType::Zstd as i32,
      sv,
      doc_state,
    };

    let mut encoded_payload = Vec::new();
    params.encode(&mut encoded_payload).map_err(|err| {
      AppError::Internal(anyhow!("Failed to encode CollabDocStateParams: {}", err))
    })?;

    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .body(Bytes::from(encoded_payload))
      .send()
      .await?;
    if resp.status().is_success() {
      let body = resp.bytes().await?;
      let decompressed_body = zstd::decode_all(Cursor::new(body))?;
      Ok(decompressed_body)
    } else {
      process_response_data::<Vec<u8>>(resp).await
    }
  }
}

struct RetryGetCollabCondition;
impl Condition<AppResponseError> for RetryGetCollabCondition {
  fn should_retry(&mut self, error: &AppResponseError) -> bool {
    !error.is_record_not_found()
  }
}

pub struct PublishCollabItemStream<Metadata, Data> {
  items: Vec<PublishCollabItem<Metadata, Data>>,
  idx: usize,
  done: bool,
}

impl<Metadata, Data> PublishCollabItemStream<Metadata, Data> {
  pub fn new(publish_collab_items: Vec<PublishCollabItem<Metadata, Data>>) -> Self {
    PublishCollabItemStream {
      items: publish_collab_items,
      idx: 0,
      done: false,
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
      if !self_mut.done {
        self_mut.done = true;
        return Poll::Ready(Some(Ok((0_u32).to_le_bytes().to_vec().into())));
      }
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

  let mut chunk = Vec::with_capacity(8 + meta.len() + d.len());
  chunk.extend_from_slice(&(meta.len() as u32).to_le_bytes()); // Encode metadata length
  chunk.extend_from_slice(&meta);
  chunk.extend_from_slice(&(d.len() as u32).to_le_bytes()); // Encode data length
  chunk.extend_from_slice(d);

  Ok(Bytes::from(chunk))
}

pub(crate) struct GetCollabAction {
  client: Client,
  params: QueryCollabParams,
}

impl GetCollabAction {
  pub fn new(client: Client, params: QueryCollabParams) -> Self {
    Self { client, params }
  }
}

impl Action for GetCollabAction {
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + Send + Sync>>;
  type Item = CollabResponse;
  type Error = AppResponseError;

  fn run(&mut self) -> Self::Future {
    let client = self.client.clone();
    let params = self.params.clone();
    let collab_type = self.params.collab_type;

    Box::pin(async move {
      let url = format!(
        "{}/api/workspace/v1/{}/collab/{}",
        client.base_url, &params.workspace_id, &params.object_id
      );
      let resp = client
        .http_client_with_auth(Method::GET, &url)
        .await?
        .query(&CollabTypeParam { collab_type })
        .send()
        .await?;
      process_response_data::<CollabResponse>(resp).await
    })
  }
}
