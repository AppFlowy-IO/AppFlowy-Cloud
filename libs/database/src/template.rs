use app_error::AppError;
use database_entity::dto::{AccountLink, TemplateCategory, TemplateCategoryType, TemplateCreator};
use sqlx::{Executor, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::pg_row::{
  AFTemplateCategoryRow, AFTemplateCategoryTypeColumn, AFTemplateCreatorRow, AccountLinkColumn,
};

pub async fn insert_new_template_category<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  name: &str,
  description: &str,
  icon: &str,
  bg_color: &str,
  category_type: TemplateCategoryType,
  priority: i32,
) -> Result<TemplateCategory, AppError> {
  let category_type_column: AFTemplateCategoryTypeColumn = category_type.into();
  let new_template_category = sqlx::query_as!(
    TemplateCategory,
    r#"
    INSERT INTO af_template_category (name, description, icon, bg_color, category_type, priority)
    VALUES ($1, $2, $3, $4, $5, $6)
    RETURNING
      category_id AS id,
      name,
      description,
      icon,
      bg_color,
      category_type AS "category_type: AFTemplateCategoryTypeColumn",
      priority
    "#,
    name,
    description,
    icon,
    bg_color,
    category_type_column as AFTemplateCategoryTypeColumn,
    priority,
  )
  .fetch_one(executor)
  .await?;
  Ok(new_template_category)
}

#[allow(clippy::too_many_arguments)]
pub async fn update_template_category_by_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  id: Uuid,
  name: &str,
  description: &str,
  icon: &str,
  bg_color: &str,
  category_type: TemplateCategoryType,
  priority: i32,
) -> Result<TemplateCategory, AppError> {
  let category_type_column: AFTemplateCategoryTypeColumn = category_type.into();
  let new_template_category = sqlx::query_as!(
    TemplateCategory,
    r#"
    UPDATE af_template_category
    SET
      name = $2,
      description = $3,
      icon = $4,
      bg_color = $5,
      category_type = $6,
      priority = $7,
      updated_at = NOW()
    WHERE category_id = $1
    RETURNING
      category_id AS id,
      name,
      description,
      icon,
      bg_color,
      category_type AS "category_type: AFTemplateCategoryTypeColumn",
      priority
    "#,
    id,
    name,
    description,
    icon,
    bg_color,
    category_type_column as AFTemplateCategoryTypeColumn,
    priority,
  )
  .fetch_one(executor)
  .await?;
  Ok(new_template_category)
}

pub async fn select_template_categories<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  name_contains: Option<&str>,
  category_type: Option<TemplateCategoryType>,
) -> Result<Vec<TemplateCategory>, AppError> {
  let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
    r#"
    SELECT
      category_id AS id,
      name,
      description,
      icon,
      bg_color,
      category_type,
      priority
    FROM af_template_category
    WHERE TRUE
    "#,
  );
  if let Some(category_type) = category_type {
    let category_type_column: AFTemplateCategoryTypeColumn = category_type.into();
    query_builder.push(" AND category_type = ");
    query_builder.push_bind(category_type_column);
  };
  if let Some(name_contains) = name_contains {
    query_builder.push(" AND name ILIKE CONCAT('%', ");
    query_builder.push_bind(name_contains);
    query_builder.push(" , '%')");
  };
  query_builder.push(" ORDER BY priority DESC, created_at ASC");
  let query = query_builder.build_query_as::<AFTemplateCategoryRow>();

  let category_rows: Vec<AFTemplateCategoryRow> = query.fetch_all(executor).await?;
  let categories = category_rows.into_iter().map(|row| row.into()).collect();

  Ok(categories)
}

pub async fn select_template_category_by_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  category_id: Uuid,
) -> Result<TemplateCategory, AppError> {
  let category = sqlx::query_as!(
    TemplateCategory,
    r#"
    SELECT
      category_id AS id,
      name,
      description,
      icon,
      bg_color,
      category_type AS "category_type: AFTemplateCategoryTypeColumn",
      priority
    FROM af_template_category
    WHERE category_id = $1
    "#,
    category_id,
  )
  .fetch_one(executor)
  .await?;
  Ok(category)
}

pub async fn delete_template_category_by_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  category_id: Uuid,
) -> Result<(), AppError> {
  let rows_affected = sqlx::query!(
    r#"
    DELETE FROM af_template_category
    WHERE category_id = $1
    "#,
    category_id,
  )
  .execute(executor)
  .await?
  .rows_affected();
  if rows_affected == 0 {
    tracing::error!(
      "No template category with id {} was found to delete",
      category_id
    );
  }
  Ok(())
}

pub async fn insert_template_creator<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  name: &str,
  avatar_url: &str,
  account_links: &[AccountLink],
) -> Result<TemplateCreator, AppError> {
  let link_types: Vec<String> = account_links
    .iter()
    .map(|link| link.link_type.clone())
    .collect();
  let url: Vec<String> = account_links.iter().map(|link| link.url.clone()).collect();
  let new_template_creator_row = sqlx::query_as!(
    AFTemplateCreatorRow,
    r#"
    WITH
      new_creator AS (
        INSERT INTO af_template_creator (name, avatar_url)
        VALUES ($1, $2)
        RETURNING creator_id, name, avatar_url
      ),
      account_links AS (
        INSERT INTO af_template_creator_account_link (creator_id, link_type, url)
        SELECT new_creator.creator_id as creator_id, link_type, url FROM
        UNNEST($3::text[], $4::text[]) AS t(link_type, url)
        CROSS JOIN new_creator
        RETURNING
          creator_id,
          link_type,
          url
      )
    SELECT
      new_creator.creator_id AS id,
      name,
      avatar_url,
      ARRAY_AGG((link_type, url)) FILTER (WHERE link_type IS NOT NULL) AS "account_links: Vec<AccountLinkColumn>"
      FROM new_creator
      LEFT OUTER JOIN account_links
      ON new_creator.creator_id = account_links.creator_id
      GROUP BY (id, name, avatar_url)
    "#,
    name,
    avatar_url,
    link_types.as_slice(),
    url.as_slice(),
  )
  .fetch_one(executor)
  .await?;
  let new_template_creator = new_template_creator_row.into();
  Ok(new_template_creator)
}

pub async fn update_template_creator_by_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  creator_id: Uuid,
  name: &str,
  avatar_url: &str,
  account_links: &[AccountLink],
) -> Result<TemplateCreator, AppError> {
  let link_types: Vec<String> = account_links
    .iter()
    .map(|link| link.link_type.clone())
    .collect();
  let url: Vec<String> = account_links.iter().map(|link| link.url.clone()).collect();
  let updated_template_creator_row = sqlx::query_as!(
    AFTemplateCreatorRow,
    r#"
    WITH
      updated_creator AS (
        UPDATE af_template_creator
          SET name = $2, avatar_url = $3, updated_at = NOW()
          WHERE creator_id = $1
        RETURNING creator_id, name, avatar_url
      ),
      account_links AS (
        INSERT INTO af_template_creator_account_link (creator_id, link_type, url)
        SELECT updated_creator.creator_id as creator_id, link_type, url FROM
        UNNEST($4::text[], $5::text[]) AS t(link_type, url)
        CROSS JOIN updated_creator
        RETURNING
          creator_id,
          link_type,
          url
      )
    SELECT
      updated_creator.creator_id AS id,
      name,
      avatar_url,
      ARRAY_AGG((link_type, url)) FILTER (WHERE link_type IS NOT NULL) AS "account_links: Vec<AccountLinkColumn>"
      FROM updated_creator
      LEFT OUTER JOIN account_links
      ON updated_creator.creator_id = account_links.creator_id
      GROUP BY (id, name, avatar_url)
    "#,
    creator_id,
    name,
    avatar_url,
    link_types.as_slice(),
    url.as_slice(),
  )
  .fetch_one(executor)
  .await?;
  let updated_template_creator = updated_template_creator_row.into();
  Ok(updated_template_creator)
}

pub async fn delete_template_creator_account_links<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  creator_id: Uuid,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
    DELETE FROM af_template_creator_account_link
    WHERE creator_id = $1
    "#,
    creator_id,
  )
  .execute(executor)
  .await?;
  Ok(())
}

pub async fn select_template_creators_by_name<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  substr_match: &str,
) -> Result<Vec<TemplateCreator>, AppError> {
  let creator_rows = sqlx::query_as!(
    AFTemplateCreatorRow,
    r#"
    SELECT
      tc.creator_id AS "id!",
      name AS "name!",
      avatar_url AS "avatar_url!",
      ARRAY_AGG((al.link_type, al.url)) FILTER (WHERE link_type IS NOT NULL) AS "account_links: Vec<AccountLinkColumn>"
    FROM af_template_creator tc
    LEFT OUTER JOIN af_template_creator_account_link al
    ON tc.creator_id = al.creator_id
    WHERE name LIKE $1
    GROUP BY (tc.creator_id, name, avatar_url)
    ORDER BY created_at ASC
    "#,
    substr_match
  )
  .fetch_all(executor)
  .await?;
  let creators = creator_rows.into_iter().map(|row| row.into()).collect();
  Ok(creators)
}

pub async fn select_template_creator_by_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  creator_id: Uuid,
) -> Result<TemplateCreator, AppError> {
  let creator_row = sqlx::query_as!(
    AFTemplateCreatorRow,
    r#"
    SELECT
      tc.creator_id AS "id!",
      name AS "name!",
      avatar_url AS "avatar_url!",
      ARRAY_AGG((al.link_type, al.url)) FILTER (WHERE link_type IS NOT NULL) AS "account_links: Vec<AccountLinkColumn>"
    FROM af_template_creator tc
    LEFT OUTER JOIN af_template_creator_account_link al
    ON tc.creator_id = al.creator_id
    WHERE tc.creator_id = $1
    GROUP BY (tc.creator_id, name, avatar_url)
    "#,
    creator_id
  )
  .fetch_one(executor)
  .await?;
  let creator = creator_row.into();
  Ok(creator)
}

pub async fn delete_template_creator_by_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  creator_id: Uuid,
) -> Result<(), AppError> {
  let rows_affected = sqlx::query!(
    r#"
    DELETE FROM af_template_creator
    WHERE creator_id = $1
    "#,
    creator_id,
  )
  .execute(executor)
  .await?
  .rows_affected();
  if rows_affected == 0 {
    tracing::error!(
      "No template creator with id {} was found to delete",
      creator_id
    );
  }
  Ok(())
}
