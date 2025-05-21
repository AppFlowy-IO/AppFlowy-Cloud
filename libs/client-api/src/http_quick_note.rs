use client_api_entity::{
  CreateQuickNoteParams, ListQuickNotesQueryParams, QuickNote, QuickNotes, UpdateQuickNoteParams,
};
use reqwest::Method;
use shared_entity::response::AppResponseError;
use uuid::Uuid;

use crate::{Client, process_response_data, process_response_error};

fn quick_note_resources_url(base_url: &str, workspace_id: Uuid) -> String {
  format!("{base_url}/api/workspace/{workspace_id}/quick-note")
}

fn quick_note_resource_url(base_url: &str, workspace_id: Uuid, quick_note_id: Uuid) -> String {
  let quick_note_resources_prefix = quick_note_resources_url(base_url, workspace_id);
  format!("{quick_note_resources_prefix}/{quick_note_id}")
}

// Quick Note API
impl Client {
  pub async fn create_quick_note(
    &self,
    workspace_id: Uuid,
    data: Option<serde_json::Value>,
  ) -> Result<QuickNote, AppResponseError> {
    let url = quick_note_resources_url(&self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&CreateQuickNoteParams { data })
      .send()
      .await?;
    process_response_data::<QuickNote>(resp).await
  }

  pub async fn list_quick_notes(
    &self,
    workspace_id: Uuid,
    search_term: Option<String>,
    offset: Option<i32>,
    limit: Option<i32>,
  ) -> Result<QuickNotes, AppResponseError> {
    let url = quick_note_resources_url(&self.base_url, workspace_id);
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .query(&ListQuickNotesQueryParams {
        search_term,
        offset,
        limit,
      })
      .send()
      .await?;
    process_response_data::<QuickNotes>(resp).await
  }

  pub async fn update_quick_note(
    &self,
    workspace_id: Uuid,
    quick_note_id: Uuid,
    data: serde_json::Value,
  ) -> Result<(), AppResponseError> {
    let url = quick_note_resource_url(&self.base_url, workspace_id, quick_note_id);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&UpdateQuickNoteParams { data })
      .send()
      .await?;
    process_response_error(resp).await
  }

  pub async fn delete_quick_note(
    &self,
    workspace_id: Uuid,
    quick_note_id: Uuid,
  ) -> Result<(), AppResponseError> {
    let url = quick_note_resource_url(&self.base_url, workspace_id, quick_note_id);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;
    process_response_error(resp).await
  }
}
