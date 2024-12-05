use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use collab::entity::{EncodedCollab, EncoderVersion};
use collab_entity::CollabType;
use sqlx::PgPool;
use tracing::{debug, error, trace, warn};
use validator::Validate;

use app_error::AppError;
use database::collab::{
  get_all_collab_snapshot_meta, latest_snapshot_time, select_snapshot, AppResult,
  COLLAB_SNAPSHOT_LIMIT, SNAPSHOT_PER_HOUR,
};
use database::file::s3_client_impl::AwsS3BucketClientImpl;
use database::file::{BucketClient, ResponseBlob};
use database::history::ops::get_latest_snapshot;
use database_entity::dto::{
  AFSnapshotMeta, AFSnapshotMetas, InsertSnapshotParams, SnapshotData, ZSTD_COMPRESSION_LEVEL,
};

use crate::metrics::CollabMetrics;

pub const SNAPSHOT_TICK_INTERVAL: Duration = Duration::from_secs(2);

fn collab_snapshot_key(workspace_id: &str, object_id: &str, snapshot_id: i64) -> String {
  let snapshot_id = u64::MAX - snapshot_id as u64;
  format!(
    "collabs/{}/{}/snapshot_{:16x}.v1.zstd",
    workspace_id, object_id, snapshot_id
  )
}

fn collab_snapshot_prefix(workspace_id: &str, object_id: &str) -> String {
  format!("collabs/{}/{}/snapshot_", workspace_id, object_id)
}

fn get_timestamp(object_key: &str) -> Option<DateTime<Utc>> {
  let (_, right) = object_key.rsplit_once('/')?;
  let trimmed = right
    .trim_start_matches("snapshot_")
    .trim_end_matches(".v1.zstd");
  let snapshot_id = u64::from_str_radix(trimmed, 16).ok()?;
  let snapshot_id = u64::MAX - snapshot_id;
  DateTime::from_timestamp_millis(snapshot_id as i64)
}

fn get_meta(objct_key: String) -> Option<AFSnapshotMeta> {
  let (left, right) = objct_key.rsplit_once('/')?;
  let (_, object_id) = left.rsplit_once('/')?;
  let trimmed = right
    .trim_start_matches("snapshot_")
    .trim_end_matches(".v1.zstd");
  let snapshot_id = u64::from_str_radix(trimmed, 16).ok()?;
  let snapshot_id = u64::MAX - snapshot_id;
  Some(AFSnapshotMeta {
    snapshot_id: snapshot_id as i64,
    object_id: object_id.to_string(),
    created_at: DateTime::from_timestamp_millis(snapshot_id as i64)?,
  })
}

#[derive(Clone)]
pub struct SnapshotControl {
  pg_pool: PgPool,
  s3: AwsS3BucketClientImpl,
  collab_metrics: Arc<CollabMetrics>,
}

impl SnapshotControl {
  pub async fn new(
    pg_pool: PgPool,
    s3: AwsS3BucketClientImpl,
    collab_metrics: Arc<CollabMetrics>,
  ) -> Self {
    Self {
      pg_pool,
      s3,
      collab_metrics,
    }
  }

  pub async fn should_create_snapshot(
    &self,
    workspace_id: &str,
    oid: &str,
  ) -> Result<bool, AppError> {
    if oid.is_empty() {
      warn!("unexpected empty object id when checking should_create_snapshot");
      return Ok(false);
    }

    let latest_created_at = self.latest_snapshot_time(workspace_id, oid).await?;
    // Subtracting a fixed duration that is known not to cause underflow. If `checked_sub_signed` returns `None`,
    // it indicates an error in calculation, thus defaulting to creating a snapshot just in case.
    let threshold_time = Utc::now().checked_sub_signed(chrono::Duration::hours(SNAPSHOT_PER_HOUR));

    match (latest_created_at, threshold_time) {
      // Return true if the latest snapshot is older than the threshold time, indicating a new snapshot should be created.
      (Some(time), Some(threshold_time)) => {
        trace!(
          "latest snapshot time: {}, threshold time: {}",
          time,
          threshold_time
        );
        Ok(time < threshold_time)
      },
      // If there's no latest snapshot time available, assume a snapshot should be created.
      _ => Ok(true),
    }
  }

  pub async fn create_snapshot(&self, params: InsertSnapshotParams) -> AppResult<AFSnapshotMeta> {
    params.validate()?;

    debug!("create snapshot for object:{}", params.object_id);
    self.collab_metrics.write_snapshot.inc();

    let timestamp = Utc::now();
    let snapshot_id = timestamp.timestamp_millis();
    let key = collab_snapshot_key(&params.workspace_id, &params.object_id, snapshot_id);
    let compressed = zstd::encode_all(params.data.as_ref(), ZSTD_COMPRESSION_LEVEL)?;
    if let Err(err) = self.s3.put_blob(&key, compressed.into(), None).await {
      self.collab_metrics.write_snapshot_failures.inc();
      return Err(err);
    }

    // drop old snapshots if exceeds limit
    let list = self
      .s3
      .list_dir(
        &collab_snapshot_prefix(&params.workspace_id, &params.object_id),
        100,
      )
      .await?;

    if list.len() > COLLAB_SNAPSHOT_LIMIT as usize {
      debug!(
        "drop {} snapshots for `{}`",
        list.len() - COLLAB_SNAPSHOT_LIMIT as usize,
        params.object_id
      );
      let trimmed: Vec<_> = list
        .into_iter()
        .skip(COLLAB_SNAPSHOT_LIMIT as usize)
        .collect();

      self.s3.delete_blobs(trimmed).await?;
    }

    Ok(AFSnapshotMeta {
      snapshot_id,
      object_id: params.object_id,
      created_at: timestamp,
    })
  }

  pub async fn get_collab_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    snapshot_id: &i64,
  ) -> AppResult<SnapshotData> {
    let key = collab_snapshot_key(workspace_id, object_id, *snapshot_id);
    match self.s3.get_blob(&key).await {
      Ok(resp) => {
        self.collab_metrics.read_snapshot.inc();
        let decompressed = zstd::decode_all(&*resp.to_blob())?;
        let encoded_collab = EncodedCollab {
          state_vector: Default::default(),
          doc_state: decompressed.into(),
          version: EncoderVersion::V1,
        };
        Ok(SnapshotData {
          object_id: object_id.to_string(),
          encoded_collab_v1: encoded_collab.encode_to_bytes()?,
          workspace_id: workspace_id.to_string(),
        })
      },
      Err(AppError::RecordNotFound(_)) => {
        debug!(
          "snapshot {} for `{}` not found in s3: fallback to postgres",
          snapshot_id, object_id
        );
        match select_snapshot(&self.pg_pool, workspace_id, object_id, snapshot_id).await? {
          None => Err(AppError::RecordNotFound(format!(
            "Can't find the snapshot with id:{}",
            snapshot_id
          ))),
          Some(row) => Ok(SnapshotData {
            object_id: object_id.to_string(),
            encoded_collab_v1: row.blob,
            workspace_id: workspace_id.to_string(),
          }),
        }
      },
      Err(err) => Err(err),
    }
  }

  /// Returns list of snapshots for given object_id in descending order of creation time.
  pub async fn get_collab_snapshot_list(
    &self,
    workspace_id: &str,
    oid: &str,
  ) -> AppResult<AFSnapshotMetas> {
    let snapshot_prefix = collab_snapshot_prefix(workspace_id, oid);
    let resp = self
      .s3
      .list_dir(&snapshot_prefix, COLLAB_SNAPSHOT_LIMIT as usize)
      .await?;
    if resp.is_empty() {
      let metas = get_all_collab_snapshot_meta(&self.pg_pool, oid).await?;
      Ok(metas)
    } else {
      let metas: Vec<_> = resp.into_iter().filter_map(get_meta).collect();
      Ok(AFSnapshotMetas(metas))
    }
  }

  pub async fn queue_snapshot(&self, params: InsertSnapshotParams) -> Result<(), AppError> {
    params.validate()?;
    trace!("Queuing snapshot for {}", params.object_id);
    let ctrl = self.clone();
    tokio::spawn(async move {
      if let Err(err) = ctrl.create_snapshot(params).await {
        error!("Failed to create snapshot: {}", err);
      }
    });
    Ok(())
  }

  pub async fn get_snapshot(
    &self,
    workspace_id: &str,
    object_id: &str,
    snapshot_id: &i64,
  ) -> Result<SnapshotData, AppError> {
    self
      .get_collab_snapshot(workspace_id, object_id, snapshot_id)
      .await
  }

  pub async fn get_latest_snapshot(
    &self,
    workspace_id: &str,
    oid: &str,
    collab_type: CollabType,
  ) -> Result<Option<SnapshotData>, AppError> {
    let snapshot_prefix = collab_snapshot_prefix(workspace_id, oid);
    let mut resp = self.s3.list_dir(&snapshot_prefix, 1).await?;
    if let Some(key) = resp.pop() {
      let resp = self.s3.get_blob(&key).await?;
      let decompressed = zstd::decode_all(&*resp.to_blob())?;
      let encoded_collab = EncodedCollab {
        state_vector: Default::default(),
        doc_state: decompressed.into(),
        version: EncoderVersion::V1,
      };
      Ok(Some(SnapshotData {
        object_id: oid.to_string(),
        encoded_collab_v1: encoded_collab.encode_to_bytes()?,
        workspace_id: workspace_id.to_string(),
      }))
    } else {
      let snapshot = get_latest_snapshot(oid, &collab_type, &self.pg_pool).await?;
      Ok(
        snapshot
          .and_then(|row| row.snapshot_meta)
          .map(|meta| SnapshotData {
            object_id: oid.to_string(),
            encoded_collab_v1: meta.snapshot,
            workspace_id: workspace_id.to_string(),
          }),
      )
    }
  }

  async fn latest_snapshot_time(
    &self,
    workspace_id: &str,
    oid: &str,
  ) -> Result<Option<DateTime<Utc>>, AppError> {
    let snapshot_prefix = collab_snapshot_prefix(workspace_id, oid);
    let mut resp = self.s3.list_dir(&snapshot_prefix, 1).await?;
    if let Some(key) = resp.pop() {
      Ok(get_timestamp(&key))
    } else {
      Ok(latest_snapshot_time(oid, &self.pg_pool).await?)
    }
  }
}
