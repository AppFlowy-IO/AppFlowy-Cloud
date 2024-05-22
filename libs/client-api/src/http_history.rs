use crate::http::log_request_id;
use crate::Client;
use collab_entity::CollabType;
use reqwest::Method;
use shared_entity::dto::history_dto::{RepeatedSnapshotMeta, SnapshotInfo};
use shared_entity::response::{AppResponse, AppResponseError};

impl Client {
  pub async fn get_snapshots(
    &self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Result<RepeatedSnapshotMeta, AppResponseError> {
    let collab_type = collab_type.value();
    let url = format!(
      "{}/api/history/{workspace_id}/{object_id}/{collab_type}",
      self.base_url,
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<RepeatedSnapshotMeta>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_latest_history(
    &self,
    workspace_id: &str,
    object_id: &str,
    collab_type: CollabType,
  ) -> Result<SnapshotInfo, AppResponseError> {
    let collab_type = collab_type.value();
    let url = format!(
      "{}/api/history/{workspace_id}/{object_id}/{collab_type}/latest",
      self.base_url,
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    log_request_id(&resp);
    AppResponse::<SnapshotInfo>::from_response(resp)
      .await?
      .into_data()
  }
}
