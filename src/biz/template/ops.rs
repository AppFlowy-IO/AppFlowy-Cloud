use std::ops::DerefMut;

use anyhow::Context;
use database::template::*;
use database_entity::dto::{
  AccountLink, Template, TemplateCategory, TemplateCategoryType, TemplateCreator, TemplateHomePage,
  TemplateMinimal,
};
use shared_entity::response::AppResponseError;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn create_new_template_category(
  pg_pool: &PgPool,
  name: &str,
  description: &str,
  icon: &str,
  bg_color: &str,
  category_type: TemplateCategoryType,
  rank: i32,
) -> Result<TemplateCategory, AppResponseError> {
  let new_template_category = insert_new_template_category(
    pg_pool,
    name,
    description,
    icon,
    bg_color,
    category_type,
    rank,
  )
  .await?;
  Ok(new_template_category)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_template_category(
  pg_pool: &PgPool,
  category_id: Uuid,
  name: &str,
  description: &str,
  icon: &str,
  bg_color: &str,
  category_type: TemplateCategoryType,
  rank: i32,
) -> Result<TemplateCategory, AppResponseError> {
  let updated_template_category = update_template_category_by_id(
    pg_pool,
    category_id,
    name,
    description,
    icon,
    bg_color,
    category_type,
    rank,
  )
  .await?;
  Ok(updated_template_category)
}

pub async fn get_template_categories(
  pg_pool: &PgPool,
  name_contains: Option<&str>,
  category_type: Option<TemplateCategoryType>,
) -> Result<Vec<TemplateCategory>, AppResponseError> {
  let categories = select_template_categories(pg_pool, name_contains, category_type).await?;
  Ok(categories)
}

pub async fn get_template_category(
  pg_pool: &PgPool,
  category_id: Uuid,
) -> Result<TemplateCategory, AppResponseError> {
  let category = select_template_category_by_id(pg_pool, category_id).await?;
  Ok(category)
}

pub async fn delete_template_category(
  pg_pool: &PgPool,
  category_id: Uuid,
) -> Result<(), AppResponseError> {
  delete_template_category_by_id(pg_pool, category_id).await?;
  Ok(())
}

pub async fn create_new_template_creator(
  pg_pool: &PgPool,
  name: &str,
  avatar_url: &str,
  account_links: &[AccountLink],
) -> Result<TemplateCreator, AppResponseError> {
  let new_template_creator =
    insert_template_creator(pg_pool, name, avatar_url, account_links).await?;
  Ok(new_template_creator)
}

pub async fn update_template_creator(
  pg_pool: &PgPool,
  creator_id: Uuid,
  name: &str,
  avatar_url: &str,
  account_links: &[AccountLink],
) -> Result<TemplateCreator, AppResponseError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to update template creator")?;
  delete_template_creator_account_links(txn.deref_mut(), creator_id).await?;
  let updated_template_creator =
    update_template_creator_by_id(txn.deref_mut(), creator_id, name, avatar_url, account_links)
      .await?;
  txn
    .commit()
    .await
    .context("Commit transaction to update template creator")?;
  Ok(updated_template_creator)
}

pub async fn get_template_creators(
  pg_pool: &PgPool,
  keyword: &Option<String>,
) -> Result<Vec<TemplateCreator>, AppResponseError> {
  let substr_match = keyword.as_deref().unwrap_or("%");
  let creators = select_template_creators_by_name(pg_pool, substr_match).await?;
  Ok(creators)
}

pub async fn get_template_creator(
  pg_pool: &PgPool,
  creator_id: Uuid,
) -> Result<TemplateCreator, AppResponseError> {
  let creator = select_template_creator_by_id(pg_pool, creator_id).await?;
  Ok(creator)
}

pub async fn delete_template_creator(
  pg_pool: &PgPool,
  creator_id: Uuid,
) -> Result<(), AppResponseError> {
  delete_template_creator_by_id(pg_pool, creator_id).await?;
  Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn create_new_template(
  pg_pool: &PgPool,
  view_id: Uuid,
  name: &str,
  description: &str,
  about: &str,
  view_url: &str,
  creator_id: Uuid,
  is_new_template: bool,
  is_featured: bool,
  category_ids: &[Uuid],
  related_view_ids: &[Uuid],
) -> Result<Template, AppResponseError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to create template creator")?;
  insert_template_view(
    txn.deref_mut(),
    view_id,
    name,
    description,
    about,
    view_url,
    creator_id,
    is_new_template,
    is_featured,
  )
  .await?;
  insert_template_view_template_category(txn.deref_mut(), view_id, category_ids).await?;
  insert_related_templates(txn.deref_mut(), view_id, related_view_ids).await?;
  let new_template = select_template_view_by_id(txn.deref_mut(), view_id).await?;
  txn
    .commit()
    .await
    .context("Commit transaction to update template creator")?;
  Ok(new_template)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_template(
  pg_pool: &PgPool,
  view_id: Uuid,
  name: &str,
  description: &str,
  about: &str,
  view_url: &str,
  creator_id: Uuid,
  is_new_template: bool,
  is_featured: bool,
  category_ids: &[Uuid],
  related_view_ids: &[Uuid],
) -> Result<Template, AppResponseError> {
  let mut txn = pg_pool
    .begin()
    .await
    .context("Begin transaction to update template")?;
  delete_template_view_template_categories(txn.deref_mut(), view_id).await?;
  delete_related_templates(txn.deref_mut(), view_id).await?;
  update_template_view(
    txn.deref_mut(),
    view_id,
    name,
    description,
    about,
    view_url,
    creator_id,
    is_new_template,
    is_featured,
  )
  .await?;
  insert_template_view_template_category(txn.deref_mut(), view_id, category_ids).await?;
  insert_related_templates(txn.deref_mut(), view_id, related_view_ids).await?;
  let updated_template = select_template_view_by_id(txn.deref_mut(), view_id).await?;
  txn
    .commit()
    .await
    .context("Commit transaction to update template")?;
  Ok(updated_template)
}

pub async fn get_templates(
  pg_pool: &PgPool,
  category_id: Option<Uuid>,
  is_featured: Option<bool>,
  is_new_template: Option<bool>,
  name_contains: Option<&str>,
) -> Result<Vec<TemplateMinimal>, AppResponseError> {
  let templates = select_templates(
    pg_pool,
    category_id,
    is_featured,
    is_new_template,
    name_contains,
    None,
  )
  .await?;
  Ok(templates)
}

pub async fn get_template(pg_pool: &PgPool, view_id: Uuid) -> Result<Template, AppResponseError> {
  let template = select_template_view_by_id(pg_pool, view_id).await?;
  Ok(template)
}

pub async fn delete_template(pg_pool: &PgPool, view_id: Uuid) -> Result<(), AppResponseError> {
  delete_template_by_view_id(pg_pool, view_id).await?;
  Ok(())
}

const DEFAULT_HOMEPAGE_CATEGORY_COUNT: i64 = 10;

pub async fn get_template_homepage(
  pg_pool: &PgPool,
  per_count: Option<i64>,
) -> Result<TemplateHomePage, AppResponseError> {
  let per_count = per_count.unwrap_or(DEFAULT_HOMEPAGE_CATEGORY_COUNT);
  let template_groups = select_template_homepage(pg_pool, per_count).await?;
  let featured_templates =
    select_templates(pg_pool, None, Some(true), None, None, Some(per_count)).await?;
  let new_templates =
    select_templates(pg_pool, None, None, Some(true), None, Some(per_count)).await?;
  let homepage = TemplateHomePage {
    template_groups,
    featured_templates,
    new_templates,
  };
  Ok(homepage)
}
