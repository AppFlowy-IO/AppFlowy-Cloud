use client_api_entity::{
  AccountLink, CreateTemplateCategoryParams, CreateTemplateCreatorParams, CreateTemplateParams,
  GetTemplateCategoriesQueryParams, GetTemplateCreatorsQueryParams, GetTemplatesQueryParams,
  Template, TemplateCategories, TemplateCategory, TemplateCategoryType, TemplateCreator,
  TemplateCreators, Templates, UpdateTemplateCategoryParams, UpdateTemplateCreatorParams,
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

fn template_resources_url(base_url: &str) -> String {
  format!("{}/template", template_api_prefix(base_url))
}

fn template_resource_url(base_url: &str, template_id: Uuid) -> String {
  format!(
    "{}/{}",
    template_creator_resources_url(base_url),
    template_id
  )
}

impl Client {
  pub async fn create_template_category(
    &self,
    name: &str,
    icon: &str,
    bg_color: &str,
    description: &str,
    category_type: TemplateCategoryType,
    priority: i32,
  ) -> Result<TemplateCategory, AppResponseError> {
    let url = category_resources_url(&self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&CreateTemplateCategoryParams {
        name: name.to_string(),
        icon: icon.to_string(),
        bg_color: bg_color.to_string(),
        description: description.to_string(),
        priority,
        category_type,
      })
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

  #[allow(clippy::too_many_arguments)]
  pub async fn update_template_category(
    &self,
    category_id: Uuid,
    name: &str,
    icon: &str,
    bg_color: &str,
    description: &str,
    category_type: TemplateCategoryType,
    priority: i32,
  ) -> Result<TemplateCategory, AppResponseError> {
    let url = category_resource_url(&self.base_url, category_id);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .json(&UpdateTemplateCategoryParams {
        name: name.to_string(),
        icon: icon.to_string(),
        bg_color: bg_color.to_string(),
        description: description.to_string(),
        category_type,
        priority,
      })
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

  #[allow(clippy::too_many_arguments)]
  pub async fn create_template(
    &self,
    view_id: Uuid,
    name: &str,
    description: &str,
    about: &str,
    view_url: &str,
    category_ids: Vec<Uuid>,
    creator_id: Uuid,
    is_new_template: bool,
    is_featured: bool,
    related_template_ids: Vec<Uuid>,
  ) -> Result<Template, AppResponseError> {
    let url = template_resources_url(&self.base_url);
    let resp = self
      .http_client_with_auth(Method::POST, &url)
      .await?
      .json(&CreateTemplateParams {
        view_id,
        name: name.to_string(),
        description: description.to_string(),
        about: about.to_string(),
        view_url: view_url.to_string(),
        category_ids,
        creator_id,
        is_new_template,
        is_featured,
        related_view_ids: related_template_ids,
      })
      .send()
      .await?;

    AppResponse::<Template>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn get_template(&self, template_id: Uuid) -> Result<Template, AppResponseError> {
    let url = template_resource_url(&self.base_url, template_id);
    let resp = self
      .http_client_without_auth(Method::GET, &url)
      .await?
      .send()
      .await?;

    AppResponse::<Template>::from_response(resp)
      .await?
      .into_data()
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

    AppResponse::<Templates>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn update_template(&self, template_id: Uuid) -> Result<Template, AppResponseError> {
    let url = template_resource_url(&self.base_url, template_id);
    let resp = self
      .http_client_with_auth(Method::PUT, &url)
      .await?
      .send()
      .await?;

    AppResponse::<Template>::from_response(resp)
      .await?
      .into_data()
  }

  pub async fn delete_template(&self, template_id: Uuid) -> Result<(), AppResponseError> {
    let url = template_resource_url(&self.base_url, template_id);
    let resp = self
      .http_client_with_auth(Method::DELETE, &url)
      .await?
      .send()
      .await?;

    AppResponse::<()>::from_response(resp).await?.into_error()
  }
}