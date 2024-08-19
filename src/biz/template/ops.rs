use std::ops::DerefMut;

use anyhow::Context;
use database::template::{
  delete_template_category_by_id, delete_template_creator_account_links,
  delete_template_creator_by_id, insert_new_template_category, insert_template_creator,
  select_template_categories, select_template_category_by_id, select_template_creator_by_id,
  select_template_creators_by_name, update_template_category_by_id, update_template_creator_by_id,
};
use database_entity::dto::{AccountLink, TemplateCategory, TemplateCategoryType, TemplateCreator};
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
