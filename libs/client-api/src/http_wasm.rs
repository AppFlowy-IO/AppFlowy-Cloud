use crate::http::RefreshTokenRet;
use crate::Client;
use app_error::AppError;
use database_entity::dto::CollabParams;
use shared_entity::response::AppResponseError;
use std::sync::atomic::Ordering;
use std::time::Duration;
use tokio_retry::strategy::FixedInterval;
use tokio_retry::RetryIf;
use tracing::{event, instrument};

impl Client {
  pub async fn create_collab_list(
    &self,
    workspace_id: &str,
    params_list: Vec<CollabParams>,
  ) -> Result<(), AppResponseError> {
    todo!()
  }

  #[instrument(level = "debug", skip_all, err)]
  pub async fn refresh_token(&self) -> Result<RefreshTokenRet, AppResponseError> {
    todo!()
  }

  async fn inner_refresh_token(&self) -> Result<(), AppResponseError> {
    todo!()
  }
}
