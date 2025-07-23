use anyhow::{anyhow, Error};
use assert_json_diff::{assert_json_matches_no_panic, Config};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::timeout;
use uuid::Uuid;

use collab::core::collab::{default_client_id, CollabOptions, DataSource};
use collab::core::collab_state::SyncState;
use collab::core::origin::CollabOrigin;
use collab::lock::RwLock;
use collab::preclude::Collab;
use collab_entity::CollabType;
use shared_entity::dto::workspace_dto::CollabResponse;
use tracing::info;

use crate::async_utils::retry_with_backoff;
use crate::test_client_config::{AssertionConfig, RetryConfig};

/// Trait for objects that can be asserted against JSON values
#[async_trait]
pub trait JsonAssertable {
  /// Gets the current JSON representation of the object
  async fn get_json(&self) -> Result<Value, Error>;

  /// Asserts that the object eventually matches the expected JSON
  async fn assert_json_eventually(
    &self,
    expected: Value,
    config: AssertionConfig,
  ) -> Result<(), Error> {
    retry_with_backoff(
      || async {
        let actual = self.get_json().await?;
        if assert_json_matches_no_panic(&actual, &expected, Config::new(config.comparison_mode))
          .is_ok()
        {
          Ok(())
        } else {
          Err(anyhow!(
            "JSON assertion failed.\nExpected: {}\nActual: {}",
            serde_json::to_string_pretty(&expected).unwrap_or_default(),
            serde_json::to_string_pretty(&actual).unwrap_or_default()
          ))
        }
      },
      RetryConfig {
        timeout: config.timeout,
        poll_interval: config.retry_interval,
        max_retries: config.max_retries,
      },
    )
    .await
  }

  /// Asserts that a specific key in the JSON eventually matches the expected value
  async fn assert_json_key_eventually(
    &self,
    key: &str,
    expected: Value,
    config: AssertionConfig,
  ) -> Result<(), Error> {
    retry_with_backoff(
      || async {
        let actual = self.get_json().await?;
        let actual_value = actual
          .get(key)
          .ok_or_else(|| anyhow!("Key '{}' not found in JSON", key))?;

        if json!({key: actual_value}) == expected {
          Ok(())
        } else {
          Err(anyhow!(
            "JSON key '{}' assertion failed.\nExpected: {}\nActual: {}",
            key,
            serde_json::to_string_pretty(&expected).unwrap_or_default(),
            serde_json::to_string_pretty(&actual).unwrap_or_default()
          ))
        }
      },
      RetryConfig {
        timeout: config.timeout,
        poll_interval: config.retry_interval,
        max_retries: config.max_retries,
      },
    )
    .await
  }
}

/// Asserts that a server collab eventually matches the expected JSON
pub async fn assert_server_collab_eventually(
  client: &mut client_api::Client,
  workspace_id: Uuid,
  object_id: Uuid,
  collab_type: CollabType,
  expected: Value,
  config: AssertionConfig,
) -> Result<(), Error> {
  retry_with_backoff(
    || async {
      let response = client
        .get_collab(database_entity::dto::QueryCollabParams::new(
          object_id,
          collab_type,
          workspace_id,
        ))
        .await
        .map_err(|e| anyhow!("Failed to get collab: {}", e))?;

      let json = collab_response_to_json(&response, object_id)?;

      if assert_json_matches_no_panic(&json, &expected, Config::new(config.comparison_mode)).is_ok()
      {
        info!(
          "Server collab assertion passed.\nExpected: {}\nActual: {}",
          serde_json::to_string_pretty(&expected).unwrap_or_default(),
          serde_json::to_string_pretty(&json).unwrap_or_default()
        );
        Ok(())
      } else {
        Err(anyhow!(
          "Server collab assertion failed.\nExpected: {}\nActual: {}",
          serde_json::to_string_pretty(&expected).unwrap_or_default(),
          serde_json::to_string_pretty(&json).unwrap_or_default()
        ))
      }
    },
    RetryConfig {
      timeout: config.timeout,
      poll_interval: config.retry_interval,
      max_retries: config.max_retries,
    },
  )
  .await
}

/// Converts a CollabResponse to JSON
fn collab_response_to_json(response: &CollabResponse, object_id: Uuid) -> Result<Value, Error> {
  let source = match response.encode_collab.version {
    collab::entity::EncoderVersion::V1 => {
      DataSource::DocStateV1(response.encode_collab.doc_state.to_vec())
    },
    collab::entity::EncoderVersion::V2 => {
      DataSource::DocStateV2(response.encode_collab.doc_state.to_vec())
    },
  };

  let options =
    CollabOptions::new(object_id.to_string(), default_client_id()).with_data_source(source);

  let collab = Collab::new_with_options(CollabOrigin::Empty, options)
    .map_err(|e| anyhow!("Failed to create collab: {}", e))?;

  Ok(collab.to_json_value())
}

/// Waits for a sync state to reach SyncFinished
pub async fn wait_for_sync_complete(
  sync_state_stream: &mut tokio_stream::wrappers::WatchStream<SyncState>,
  current_state: SyncState,
  timeout_duration: Duration,
  collab: &Arc<RwLock<Collab>>,
) -> Result<(), Error> {
  if current_state == SyncState::SyncFinished {
    return Ok(());
  }

  use tokio_stream::StreamExt;
  let result = timeout(timeout_duration, async {
    while let Some(state) = sync_state_stream.next().await {
      if state == SyncState::SyncFinished {
        return Ok(());
      }
    }
    Err(anyhow!(
      "Sync state stream ended before reaching SyncFinished"
    ))
  })
  .await;

  match result {
    Ok(sync_result) => sync_result,
    Err(_) => {
      // Timeout occurred, check the actual sync state in collab
      let lock = collab.read().await;
      let actual_sync_state = lock.get_state().sync_state();
      Err(anyhow!(
        "Timeout waiting for sync to complete. Current sync state in collab: {:?}",
        actual_sync_state
      ))
    },
  }
}
