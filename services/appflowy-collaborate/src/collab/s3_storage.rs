#![allow(unused_imports)]

use crate::command::{CLCommandSender, CollaborationCommand};
use anyhow::anyhow;
use app_error::AppError;
use async_trait::async_trait;
use aws_sdk_s3::config::http::HttpResponse;
use aws_sdk_s3::operation::delete_object::{DeleteObjectError, DeleteObjectOutput};
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use bytes::Bytes;
use chrono::{DateTime, NaiveDateTime, Utc};
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use collab_rt_entity::ClientCollabMessage;
use database::collab::{
  batch_select_collab_blob, get_all_collab_snapshot_meta, select_blob_from_af_collab,
  select_snapshot, AppResult, CollabMetadata, CollabStorage, GetCollabOrigin, SNAPSHOT_PER_HOUR,
};
use database_entity::dto::{
  AFSnapshotMeta, AFSnapshotMetas, CollabParams, InsertSnapshotParams, QueryCollab,
  QueryCollabParams, QueryCollabResult, SnapshotData,
};
use itertools::Itertools;
use sqlx::{PgPool, Postgres, Transaction};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::task::JoinSet;
use tokio::{join, try_join};
use tokio_stream::StreamExt;

#[inline]
fn s3_error<E: std::error::Error>(err: E) -> AppError {
  AppError::S3ResponseError(err.to_string())
}

pub struct S3CollabStorage {
  pg_pool: PgPool,
  s3: Client,
  bucket: String,
  rt_cmd_sender: CLCommandSender,
  success_attempts: AtomicU64,
  total_attempts: AtomicU64,
}

impl S3CollabStorage {
  const CHUNK_SIZE: usize = 500;

  pub fn new(rt_cmd_sender: CLCommandSender, pg_pool: PgPool, s3: Client, bucket: String) -> Self {
    Self {
      rt_cmd_sender,
      pg_pool,
      s3,
      bucket,
      success_attempts: AtomicU64::new(0),
      total_attempts: AtomicU64::new(0),
    }
  }

  fn collab_key(workspace_id: &str, object_id: &str) -> String {
    format!("{}/{}/collab", workspace_id, object_id)
  }

  fn snapshot_key(workspace_id: &str, object_id: &str, snapshot_id: i64) -> String {
    // setup the timestamp in reverse order (the most recent will be first)
    let ts = u64::MAX - snapshot_id as u64;
    format!("{}/{}/snapshot-{:16x}", workspace_id, object_id, ts)
  }

  fn snapshot_key_prefix(workspace_id: &str, object_id: &str) -> String {
    format!("{}/{}/snapshot-", workspace_id, object_id)
  }

  fn snapshot_meta_from_key(key: &str) -> Option<AFSnapshotMeta> {
    let mut splits = key.split('/');
    let _workspace_id = splits.next()?;
    let object_id = splits.next()?;
    let snapshot = splits.next()?;
    let snapshot_id = snapshot
      .rsplitn(2, '-')
      .next()
      .and_then(|s| u64::from_str_radix(s, 16).ok())
      .map(|ts| (u64::MAX - ts) as i64)?;
    let created_at = DateTime::from_timestamp_millis(snapshot_id)?;
    Some(AFSnapshotMeta {
      snapshot_id,
      object_id: object_id.into(),
      created_at,
    })
  }

  async fn encoded_collab_s3(&self, path: String) -> AppResult<Option<Bytes>> {
    let rep = self
      .s3
      .get_object()
      .bucket(&self.bucket)
      .key(path)
      .send()
      .await
      .map_err(s3_error)?;
    let body = rep.body.collect().await.map_err(s3_error)?.into_bytes();
    Ok(Some(body))
  }

  async fn collab_pg(&self, params: QueryCollabParams) -> AppResult<EncodedCollab> {
    let blob =
      select_blob_from_af_collab(&self.pg_pool, &params.collab_type, &params.object_id).await?;
    Ok(EncodedCollab::decode_from_bytes(&blob)?)
  }

  async fn snapshot_list_pg(
    &self,
    workspace_id: &str,
    object_id: &str,
    max_count: usize,
  ) -> AppResult<Vec<AFSnapshotMeta>> {
    let metas = get_all_collab_snapshot_meta(&self.pg_pool, object_id).await?;
    Ok(metas.0)
  }

  async fn snapshot_pg(
    &self,
    workspace_id: &str,
    object_id: &str,
    snapshot_id: &i64,
  ) -> AppResult<Bytes> {
    if let Some(row) = select_snapshot(&self.pg_pool, snapshot_id).await? {
      Ok(row.blob.into())
    } else {
      Err(AppError::RecordNotFound(format!(
        "Snapshot not found: {}",
        snapshot_id
      )))
    }
  }

  async fn delete_collab_s3(&self, workspace_id: &str, object_id: &str) -> AppResult<()> {
    let rep = self
      .s3
      .delete_object()
      .bucket(&self.bucket)
      .key(Self::collab_key(workspace_id, object_id))
      .send()
      .await
      .map_err(s3_error)?;
    Ok(())
  }

  async fn delete_collab_pg(&self, workspace_id: &str, object_id: &str) -> AppResult<()> {
    sqlx::query!(
      r#"
        UPDATE af_collab
        SET deleted_at = $2
        WHERE oid = $1;
        "#,
      object_id,
      chrono::Utc::now()
    )
    .execute(&self.pg_pool)
    .await?;
    Ok(())
  }

  async fn snapshot_list(
    &self,
    workspace_id: &str,
    object_id: &str,
    max_count: usize,
  ) -> AppResult<Vec<AFSnapshotMeta>> {
    let key_prefix = Self::snapshot_key_prefix(workspace_id, object_id);
    let rep = self
      .s3
      .list_objects_v2()
      .bucket(&self.bucket)
      .prefix(key_prefix)
      .max_keys(max_count as i32)
      .send()
      .await
      .map_err(s3_error)?;

    if let Some(contents) = rep.contents {
      let metas: Vec<_> = contents
        .into_iter()
        .filter_map(|obj| {
          let key = obj.key.unwrap();
          Self::snapshot_meta_from_key(&key)
        })
        .collect();
      if !metas.is_empty() {
        Ok(metas)
      } else {
        // fallback to read snapshots from postgres
        let metas = self
          .snapshot_list_pg(workspace_id, object_id, max_count)
          .await?;
        Ok(metas)
      }
    } else {
      Ok(Vec::new())
    }
  }

  async fn get_object_s3(
    s3: Client,
    bucket: String,
    workspace_id: String,
    object_id: String,
  ) -> Result<(String, Option<Bytes>), anyhow::Error> {
    let key = Self::collab_key(&workspace_id, &object_id);
    match s3.get_object().bucket(bucket).key(key).send().await {
      Ok(rep) => {
        let body = rep
          .body
          .collect()
          .await
          .map_err(anyhow::Error::from)?
          .into_bytes();
        Ok((object_id, Some(body)))
      },
      Err(err) => {
        if let Some(GetObjectError::NoSuchKey(_)) = err.as_service_error() {
          Ok((object_id, None))
        } else {
          Err(err.into())
        }
      },
    }
  }
}

#[async_trait]
impl CollabStorage for S3CollabStorage {
  fn encode_collab_redis_query_state(&self) -> (u64, u64) {
    let total = self.total_attempts.load(Ordering::Relaxed);
    let success = self.success_attempts.load(Ordering::Relaxed);
    (total, success)
  }

  async fn queue_insert_or_update_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    write_immediately: bool,
  ) -> AppResult<()> {
    let key = Self::collab_key(workspace_id, &params.object_id);
    let rep = self
      .s3
      .put_object()
      .bucket(&self.bucket)
      .key(key)
      .body(params.encoded_collab_v1.into())
      .send()
      .await
      .map_err(s3_error)?;
    Ok(())
  }

  async fn batch_insert_new_collab(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: Vec<CollabParams>,
  ) -> AppResult<()> {
    for chunk in params.chunks(Self::CHUNK_SIZE) {
      let mut join_set = JoinSet::new();
      for params in chunk {
        let key = Self::collab_key(workspace_id, &params.object_id);
        let s3 = self.s3.clone();
        let bucket = self.bucket.clone();
        let data = ByteStream::from(params.encoded_collab_v1.clone());
        join_set.spawn(s3.put_object().bucket(bucket).key(key).body(data).send());

        while let Some(res) = join_set.join_next().await {
          self.total_attempts.fetch_add(1, Ordering::Relaxed);
          match res {
            Ok(Ok(_)) => {
              self.success_attempts.fetch_add(1, Ordering::Relaxed);
            },
            Ok(Err(err)) => {
              tracing::error!("failed to put collab to s3: {}", err);
            },
            Err(err) => {
              tracing::error!("failed to join task: {}", err);
            },
          }
        }
      }
    }
    Ok(())
  }

  async fn insert_new_collab_with_transaction(
    &self,
    workspace_id: &str,
    uid: &i64,
    params: CollabParams,
    transaction: &mut Transaction<'_, Postgres>,
    action_description: &str,
  ) -> AppResult<()> {
    self
      .queue_insert_or_update_collab(workspace_id, uid, params, true)
      .await
  }

  async fn get_encode_collab(
    &self,
    origin: GetCollabOrigin,
    params: QueryCollabParams,
    from_editing_collab: bool,
  ) -> AppResult<EncodedCollab> {
    let key = Self::collab_key(&params.workspace_id, &params.object_id);
    match self.encoded_collab_s3(key).await? {
      None => {
        tracing::debug!("Failed to get collab from s3. Fallback to database");
        self.collab_pg(params).await
      },
      Some(collab) => Ok(EncodedCollab::decode_from_bytes(&collab)?),
    }
  }

  async fn broadcast_encode_collab(
    &self,
    object_id: String,
    collab_messages: Vec<ClientCollabMessage>,
  ) -> Result<(), AppError> {
    let (sender, recv) = tokio::sync::oneshot::channel();

    self
      .rt_cmd_sender
      .send(CollaborationCommand::ServerSendCollabMessage {
        object_id,
        collab_messages,
        ret: sender,
      })
      .await
      .map_err(|err| {
        AppError::Unhandled(format!(
          "Failed to send encode collab command to realtime server: {}",
          err
        ))
      })?;

    match recv.await {
          Ok(res) =>
              if let Err(err) = res {
                  tracing::error!("Failed to broadcast encode collab: {}", err);
              }
          ,
          // caller may have dropped the receiver
          Err(err) => tracing::warn!("Failed to receive response from realtime server: {}", err),
      }

    Ok(())
  }

  async fn batch_get_collab(
    &self,
    uid: &i64,
    workspace_id: &str,
    queries: Vec<QueryCollab>,
    from_editing_collab: bool,
  ) -> HashMap<String, QueryCollabResult> {
    let mut result = HashMap::new();
    let mut not_found = Vec::new();
    for chunk in queries.chunks(Self::CHUNK_SIZE) {
      let mut join_set = JoinSet::new();
      for query in chunk {
        let object_id = query.object_id.clone();
        let s3 = self.s3.clone();
        let bucket = self.bucket.clone();
        join_set.spawn(Self::get_object_s3(
          s3,
          bucket,
          workspace_id.to_string(),
          object_id,
        ));
      }

      while let Some(res) = join_set.join_next().await {
        match res {
          Ok(Ok((object_id, Some(value)))) => {
            result.insert(
              object_id,
              QueryCollabResult::Success {
                encode_collab_v1: value.into(),
              },
            );
          },
          Ok(Ok((object_id, None))) => {
            not_found.push(object_id);
          },
          Ok(Err(err)) => {
            tracing::error!("failed to get collab from s3: {}", err);
          },
          Err(err) => {
            tracing::error!("failed to join task: {}", err);
          },
        }
      }
    }

    if !not_found.is_empty() {
      let res = batch_select_collab_blob(&self.pg_pool, queries).await;
      result.extend(res);
    }

    result
  }

  async fn delete_collab(&self, workspace_id: &str, uid: &i64, object_id: &str) -> AppResult<()> {
    try_join!(
      self.delete_collab_s3(workspace_id, object_id),
      self.delete_collab_pg(workspace_id, object_id)
    )?;
    Ok(())
  }

  async fn should_create_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
  ) -> Result<bool, AppError> {
    let mut metas = self.snapshot_list(workspace_id, object_id, 1).await?;
    let threshold_time = Utc::now().checked_sub_signed(chrono::Duration::hours(SNAPSHOT_PER_HOUR));
    match (metas.pop(), threshold_time) {
      (Some(meta), Some(threshold)) => Ok(meta.created_at < threshold),
      _ => Ok(true),
    }
  }

  async fn create_snapshot(&self, params: InsertSnapshotParams) -> AppResult<AFSnapshotMeta> {
    let created_at = Utc::now();
    let snapshot_id = created_at.timestamp_millis();
    let key = Self::snapshot_key(&params.workspace_id, &params.object_id, snapshot_id);
    let rep = self
      .s3
      .put_object()
      .bucket(&self.bucket)
      .key(key)
      .body(params.encoded_collab_v1.into())
      .send()
      .await
      .map_err(s3_error)?;
    Ok(AFSnapshotMeta {
      snapshot_id,
      object_id: params.object_id,
      created_at,
    })
  }

  async fn queue_snapshot(&self, params: InsertSnapshotParams) -> AppResult<()> {
    self.create_snapshot(params).await?;
    Ok(())
  }

  async fn get_collab_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    snapshot_id: &i64,
  ) -> AppResult<SnapshotData> {
    let key = Self::snapshot_key(workspace_id, object_id, *snapshot_id);
    let encoded_collab = match self.encoded_collab_s3(key).await? {
      Some(collab) => collab,
      None => {
        tracing::debug!("Failed to get collab from s3. Fallback to database");
        self
          .snapshot_pg(workspace_id, object_id, snapshot_id)
          .await?
      },
    };

    Ok(SnapshotData {
      object_id: object_id.to_string(),
      encoded_collab_v1: encoded_collab.into(),
      workspace_id: workspace_id.to_string(),
    })
  }

  async fn get_collab_snapshot_list(
    &self,
    workspace_id: &str,
    object_id: &str,
  ) -> AppResult<AFSnapshotMetas> {
    let metas = self.snapshot_list(workspace_id, object_id, 20).await?;
    Ok(AFSnapshotMetas(metas))
  }
}

#[cfg(test)]
mod test {
  use crate::collab::s3_storage::S3CollabStorage;
  use uuid::Uuid;

  #[test]
  fn snapshot_key_format_parse() {
    let workspace_id = Uuid::new_v4().to_string();
    let object_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now();
    let snapshot_id = now.timestamp_millis();
    let snapshot_key = S3CollabStorage::snapshot_key(&workspace_id, &object_id, snapshot_id);

    let meta = S3CollabStorage::snapshot_meta_from_key(&snapshot_key).unwrap();

    assert_eq!(meta.object_id, object_id, "object id should match");
    assert_eq!(meta.snapshot_id, snapshot_id, "snapshot id should match");
    assert_eq!(meta.created_at, now, "created at should match");
  }
}
