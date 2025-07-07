use client_api_entity::{MentionablePerson, MentionablePersons, WorkspaceMemberProfile};
use reqwest::Method;
use shared_entity::response::AppResponseError;
use tracing::instrument;
use uuid::Uuid;

use crate::{process_response_data, process_response_error, Client};

impl Client {
  #[instrument(level = "info", skip_all, err)]
  pub async fn list_workspace_mentionable_persons(
    &self,
    workspace_id: &Uuid,
  ) -> Result<MentionablePersons, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/mentionable-person",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<MentionablePersons>(resp).await
  }

  #[instrument(level = "info", skip_all, err)]
  pub async fn get_workspace_mentionable_person(
    &self,
    workspace_id: &Uuid,
    person_id: &Uuid,
  ) -> Result<MentionablePerson, AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/mentionable-person/{}",
      self.base_url, workspace_id, person_id
    );
    let resp = self
      .http_client_with_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    process_response_data::<MentionablePerson>(resp).await
  }

  pub async fn update_workspace_member_profile(
    &self,
    workspace_id: &Uuid,
    updated_profile: &WorkspaceMemberProfile,
  ) -> Result<(), AppResponseError> {
    let url = format!(
      "{}/api/workspace/{}/update-member-profile",
      self.base_url, workspace_id
    );
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(updated_profile)
      .send()
      .await?;
    process_response_error(resp).await
  }
}
