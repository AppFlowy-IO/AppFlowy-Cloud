use client_api_entity::{
  AccountLink, CreateTemplateCategoryParams, CreateTemplateCreatorParams, CreateTemplateParams,
  GetTemplateCategoriesQueryParams, GetTemplateCreatorsQueryParams, GetTemplatesQueryParams,
  Template, TemplateCategories, TemplateCategory, TemplateCategoryType, TemplateCreator,
  TemplateCreators, TemplateWithPublishInfo, Templates, UpdateTemplateCategoryParams,
  UpdateTemplateCreatorParams, UpdateTemplateParams,
};
use reqwest::Method;
use shared_entity::response::AppResponseError;
use uuid::Uuid;

use crate::{Client, process_response_data, process_response_error};

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

fn template_resources_url(base_url: &str) -> String {
  format!("{}/template", template_api_prefix(base_url))
}

fn template_resource_url(base_url: &str, view_id: Uuid) -> String {
  format!("{}/{}", template_resources_url(base_url), view_id)
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

    process_response_data::<TemplateCategory>(resp).await
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
    process_response_data::<TemplateCategories>(resp).await
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
    process_response_data::<TemplateCategory>(resp).await
  }

  pub async fn delete_template_category(&self, category_id: Uuid) -> Result<(), AppResponseError> {
    let url = category_resource_url(&self.base_url, category_id);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;
    process_response_error(resp).await
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

    process_response_data::<TemplateCategory>(resp).await
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

    process_response_data::<TemplateCreator>(resp).await
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
    process_response_data::<TemplateCreators>(resp).await
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
    process_response_data::<TemplateCreator>(resp).await
  }

  pub async fn delete_template_creator(&self, creator_id: Uuid) -> Result<(), AppResponseError> {
    let url = template_creator_resource_url(&self.base_url, creator_id);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;
    process_response_error(resp).await
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

    process_response_data::<TemplateCreator>(resp).await
  }

  pub async fn create_template(
    &self,
    params: &CreateTemplateParams,
  ) -> Result<Template, AppResponseError> {
    let url = template_resources_url(&self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(params)
      .send()
      .await?;

    process_response_data::<Template>(resp).await
  }

  pub async fn get_template(
    &self,
    view_id: Uuid,
  ) -> Result<TemplateWithPublishInfo, AppResponseError> {
    let url = template_resource_url(&self.base_url, view_id);
    let resp = self
      .http_client_without_auth(Method::GET, &url)
      .await?
      .send()
      .await?;

    process_response_data::<TemplateWithPublishInfo>(resp).await
  }

  pub async fn get_templates(
    &self,
    category_id: Option<Uuid>,
    is_featured: Option<bool>,
    is_new_template: Option<bool>,
    name_contains: Option<String>,
  ) -> Result<Templates, AppResponseError> {
    let url = template_resources_url(&self.base_url);
    let resp = self
      .http_client_without_auth(Method::GET, &url)
      .await?
      .query(&GetTemplatesQueryParams {
        category_id,
        is_featured,
        is_new_template,
        name_contains,
      })
      .send()
      .await?;

    process_response_data::<Templates>(resp).await
  }

  pub async fn update_template(
    &self,
    view_id: Uuid,
    params: &UpdateTemplateParams,
  ) -> Result<Template, AppResponseError> {
    let url = template_resource_url(&self.base_url, view_id);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(params)
      .send()
      .await?;

    process_response_data::<Template>(resp).await
  }

  pub async fn delete_template(&self, view_id: Uuid) -> Result<(), AppResponseError> {
    let url = template_resource_url(&self.base_url, view_id);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;

    process_response_error(resp).await
  }
}
