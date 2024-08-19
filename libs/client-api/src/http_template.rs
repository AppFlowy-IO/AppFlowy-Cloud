use client_api_entity::{
  AccountLink, CreateTemplateCategoryParams, CreateTemplateCreatorParams,
  GetTemplateCategoriesQueryParams, GetTemplateCreatorsQueryParams, TemplateCategories,
  TemplateCategory, TemplateCategoryType, TemplateCreator, TemplateCreators,
  UpdateTemplateCategoryParams, UpdateTemplateCreatorParams,
};
use reqwest::Method;
use shared_entity::response::{AppResponse, AppResponseError};
use uuid::Uuid;

use crate::Client;

fn template_api_prefix(base_url: &str) -> String {
  format!("{}/api/template-center", base_url)
}

fn category_resources_url(base_url: &str) -> String {
  format!("{}/category", template_api_prefix(base_url))
}

fn category_resource_url(base_url: &str, category_id: Uuid) -> String {
  format!("{}/{}", category_resources_url(base_url), category_id)
}

fn template_creator_resources_url(base_url: &str) -> String {
  format!("{}/creator", template_api_prefix(base_url))
}

fn template_creator_resource_url(base_url: &str, creator_id: Uuid) -> String {
  format!(
    "{}/{}",
    template_creator_resources_url(base_url),
    creator_id
  )
}

impl Client {
  pub async fn create_template_category(
    &self,
    params: &CreateTemplateCategoryParams,
  ) -> Result<TemplateCategory, AppResponseError> {
    let url = category_resources_url(&self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;

    AppResponse::<TemplateCategory>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_template_categories(
    &self,
    name_contains: Option<&str>,
    category_type: Option<TemplateCategoryType>,
  ) -> Result<TemplateCategories, AppResponseError> {
    let url = category_resources_url(&self.base_url);
    let resp = self
      .http_client_without_auth(Method::GET, &url)
      .await?
      .query(&GetTemplateCategoriesQueryParams {
        name_contains: name_contains.map(|s| s.to_string()),
        category_type,
      })
      .send()
      .await?;
    AppResponse::<TemplateCategories>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_template_category(
    &self,
    category_id: Uuid,
  ) -> Result<TemplateCategory, AppResponseError> {
    let url = category_resource_url(&self.base_url, category_id);
    let resp = self
      .http_client_without_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<TemplateCategory>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn delete_template_category(&self, category_id: Uuid) -> Result<(), AppResponseError> {
    let url = category_resource_url(&self.base_url, category_id);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn update_template_category(
    &self,
    category_id: Uuid,
    params: &UpdateTemplateCategoryParams,
  ) -> Result<TemplateCategory, AppResponseError> {
    let url = category_resource_url(&self.base_url, category_id);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(params)
      .send()
      .await?;

    AppResponse::<TemplateCategory>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn create_template_creator(
    &self,
    name: &str,
    avatar_url: &str,
    account_links: Vec<AccountLink>,
  ) -> Result<TemplateCreator, AppResponseError> {
    let url = template_creator_resources_url(&self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&CreateTemplateCreatorParams {
        name: name.to_string(),
        avatar_url: avatar_url.to_string(),
        account_links,
      })
      .send()
      .await?;

    AppResponse::<TemplateCreator>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_template_creators(
    &self,
    name_contains: Option<&str>,
  ) -> Result<TemplateCreators, AppResponseError> {
    let url = template_creator_resources_url(&self.base_url);
    let resp = self
      .http_client_without_auth(Method::GET, &url)
      .await?
      .query(&GetTemplateCreatorsQueryParams {
        name_contains: name_contains.map(|s| s.to_string()),
      })
      .send()
      .await?;
    AppResponse::<TemplateCreators>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_template_creator(
    &self,
    creator_id: Uuid,
  ) -> Result<TemplateCreator, AppResponseError> {
    let url = template_creator_resource_url(&self.base_url, creator_id);
    let resp = self
      .http_client_without_auth(Method::GET, &url)
      .await?
      .send()
      .await?;
    AppResponse::<TemplateCreator>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn delete_template_creator(&self, creator_id: Uuid) -> Result<(), AppResponseError> {
    let url = template_creator_resource_url(&self.base_url, creator_id);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;
    AppResponse::<()>::from_response(resp).await?.into_error()
  }

  pub async fn update_template_creator(
    &self,
    creator_id: Uuid,
    name: &str,
    avatar_url: &str,
    account_links: Vec<AccountLink>,
  ) -> Result<TemplateCreator, AppResponseError> {
    let url = template_creator_resource_url(&self.base_url, creator_id);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&UpdateTemplateCreatorParams {
        name: name.to_string(),
        avatar_url: avatar_url.to_string(),
        account_links,
      })
      .send()
      .await?;

    AppResponse::<TemplateCreator>::from_response(resp)
      .await?
      .into_data()
  }
}
