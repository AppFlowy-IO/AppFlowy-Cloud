use app_error::AppError;
use database_entity::dto::{
  AccountLink, Template, TemplateCategory, TemplateCategoryType, TemplateCreator, TemplateGroup,
  TemplateMinimal,
};
use sqlx::{Executor, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::pg_row::{
  AFTemplateCategoryMinimalRow, AFTemplateCategoryRow, AFTemplateCategoryTypeColumn,
  AFTemplateCreatorRow, AFTemplateGroupRow, AFTemplateMinimalRow, AFTemplateRow, AccountLinkColumn,
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
      category_id,
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
  sqlx::query!(
    r#"
    DELETE FROM af_template_category
    WHERE category_id = $1
    "#,
    category_id,
  )
  .execute(executor)
  .await?;
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
      ARRAY_AGG((link_type, url)) FILTER (WHERE link_type IS NOT NULL) AS "account_links: Vec<AccountLinkColumn>",
      0 AS "number_of_templates!"
      FROM new_creator
      LEFT OUTER JOIN account_links
      USING (creator_id)
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
      ),
      creator_number_of_templates AS (
        SELECT
          creator_id,
          COUNT(1)::int AS number_of_templates
        FROM af_template_view
        WHERE creator_id = $1
        GROUP BY creator_id
      )
    SELECT
      updated_creator.creator_id AS id,
      name,
      avatar_url,
      ARRAY_AGG((link_type, url)) FILTER (WHERE link_type IS NOT NULL) AS "account_links: Vec<AccountLinkColumn>",
      COALESCE(number_of_templates, 0) AS "number_of_templates!"
      FROM updated_creator
      LEFT OUTER JOIN account_links
      USING (creator_id)
      LEFT OUTER JOIN creator_number_of_templates
      USING (creator_id)
      GROUP BY (id, name, avatar_url, number_of_templates)
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
    WITH creator_number_of_templates AS (
      SELECT
        creator_id,
        COUNT(1)::int AS number_of_templates
      FROM af_template_view
      WHERE name ILIKE $1
      GROUP BY creator_id
    )

    SELECT
      creator.creator_id AS "id!",
      name AS "name!",
      avatar_url AS "avatar_url!",
      ARRAY_AGG((link_type, url)) FILTER (WHERE link_type IS NOT NULL) AS "account_links: Vec<AccountLinkColumn>",
      COALESCE(number_of_templates, 0) AS "number_of_templates!"
    FROM af_template_creator creator
    LEFT OUTER JOIN af_template_creator_account_link account_link
    USING (creator_id)
    LEFT OUTER JOIN creator_number_of_templates
    USING (creator_id)
    WHERE name ILIKE $1
    GROUP BY (creator.creator_id, name, avatar_url, number_of_templates)
    ORDER BY created_at ASC
    "#,
    format!("%{}%", substr_match)
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
    WITH creator_number_of_templates AS (
      SELECT
        creator_id,
        COUNT(1)::int AS number_of_templates
      FROM af_template_view
      WHERE creator_id = $1
      GROUP BY creator_id
    )
    SELECT
      creator.creator_id AS "id!",
      name AS "name!",
      avatar_url AS "avatar_url!",
      ARRAY_AGG((link_type, url)) FILTER (WHERE link_type IS NOT NULL) AS "account_links: Vec<AccountLinkColumn>",
      COALESCE(number_of_templates, 0) AS "number_of_templates!"
    FROM af_template_creator creator
    LEFT OUTER JOIN af_template_creator_account_link account_link
    USING (creator_id)
    LEFT OUTER JOIN creator_number_of_templates
    USING (creator_id)
    WHERE creator.creator_id = $1
    GROUP BY (creator.creator_id, name, avatar_url, number_of_templates)
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
  sqlx::query!(
    r#"
    DELETE FROM af_template_creator
    WHERE creator_id = $1
    "#,
    creator_id,
  )
  .execute(executor)
  .await?;
  Ok(())
}

pub async fn insert_template_view_template_category<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  view_id: Uuid,
  category_ids: &[Uuid],
) -> Result<(), AppError> {
  let rows_affected = sqlx::query!(
    r#"
    INSERT INTO af_template_view_template_category (view_id, category_id)
    SELECT $1 as view_id, category_id FROM
    UNNEST($2::uuid[]) AS category_id
    "#,
    view_id,
    category_ids
  )
  .execute(executor)
  .await?
  .rows_affected();
  if rows_affected == 0 {
    tracing::error!(
      "at least one category id is expected to be inserted for view_id {}",
      view_id
    );
  }
  Ok(())
}

pub async fn delete_template_view_template_categories<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  view_id: Uuid,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
    DELETE FROM af_template_view_template_category
    WHERE view_id = $1
    "#,
    view_id,
  )
  .execute(executor)
  .await?;
  Ok(())
}

pub async fn insert_related_templates<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  view_id: Uuid,
  category_ids: &[Uuid],
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
    INSERT INTO af_related_template_view (view_id, related_view_id)
    SELECT $1 AS view_id, related_view_id
    FROM UNNEST($2::uuid[]) AS t(related_view_id)
    "#,
    view_id,
    category_ids
  )
  .execute(executor)
  .await?;
  Ok(())
}

pub async fn delete_related_templates<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  view_id: Uuid,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
    DELETE FROM af_related_template_view
    WHERE view_id = $1
    "#,
    view_id,
  )
  .execute(executor)
  .await?;
  Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn insert_template_view<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  view_id: Uuid,
  name: &str,
  description: &str,
  about: &str,
  view_url: &str,
  creator_id: Uuid,
  is_new_template: bool,
  is_featured: bool,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
    INSERT INTO af_template_view (
      view_id,
      name,
      description,
      about,
      view_url,
      creator_id,
      is_new_template,
      is_featured
    )
    VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
    "#,
    view_id,
    name,
    description,
    about,
    view_url,
    creator_id,
    is_new_template,
    is_featured
  )
  .execute(executor)
  .await?;
  Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn update_template_view<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  view_id: Uuid,
  name: &str,
  description: &str,
  about: &str,
  view_url: &str,
  creator_id: Uuid,
  is_new_template: bool,
  is_featured: bool,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
    UPDATE af_template_view SET
      updated_at = NOW(),
      name = $2,
      description = $3,
      about = $4,
      view_url = $5,
      creator_id = $6,
      is_new_template = $7,
      is_featured = $8
      WHERE view_id = $1
    "#,
    view_id,
    name,
    description,
    about,
    view_url,
    creator_id,
    is_new_template,
    is_featured
  )
  .execute(executor)
  .await?;
  Ok(())
}

pub async fn select_template_view_by_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  view_id: Uuid,
) -> Result<Template, AppError> {
  let view_row = sqlx::query_as!(
    AFTemplateRow,
    r#"
      WITH template_with_creator_account_link AS (
        SELECT
          template.view_id,
          template.creator_id,
          COALESCE(
            ARRAY_AGG((link_type, url)::account_link_type) FILTER (WHERE link_type IS NOT NULL),
            '{}'
          ) AS account_links
        FROM af_template_view template
        JOIN af_published_collab
        USING (view_id)
        JOIN af_template_creator creator
        USING (creator_id)
        LEFT OUTER JOIN af_template_creator_account_link account_link
        USING (creator_id)
        WHERE view_id = $1
        GROUP BY (view_id, template.creator_id)
      ),
      related_template_with_category AS (
        SELECT
          template.related_view_id,
          ARRAY_AGG(
            (
              template_category.category_id,
              template_category.name,
              template_category.icon,
              template_category.bg_color
            )::template_category_minimal_type
          ) AS categories
        FROM af_related_template_view template
        JOIN af_template_view_template_category template_template_category
        ON template.related_view_id = template_template_category.view_id
        JOIN af_template_category template_category
        USING (category_id)
        WHERE template.view_id = $1
        GROUP BY template.related_view_id
      ),
      template_with_related_template AS (
        SELECT
          template.view_id,
          ARRAY_AGG(
            (
              template.related_view_id,
              related_template.created_at,
              related_template.updated_at,
              related_template.name,
              related_template.description,
              related_template.view_url,
              (
                creator.creator_id,
                creator.name,
                creator.avatar_url
              )::template_creator_minimal_type,
              related_template_with_category.categories,
              related_template.is_new_template,
              related_template.is_featured
            )::template_minimal_type
          ) AS related_templates
        FROM af_related_template_view template
        JOIN af_template_view related_template
        ON template.related_view_id = related_template.view_id
        JOIN af_template_creator creator
        ON related_template.creator_id = creator.creator_id
        JOIN related_template_with_category
        ON template.related_view_id = related_template_with_category.related_view_id
        WHERE template.view_id = $1
        GROUP BY template.view_id
      ),
      template_with_category AS (
        SELECT
          view_id,
          COALESCE(
            ARRAY_AGG((
              vtc.category_id,
              name,
              icon,
              bg_color,
              description,
              category_type,
              priority
            )) FILTER (WHERE vtc.category_id IS NOT NULL),
            '{}'
          ) AS categories
        FROM af_template_view_template_category vtc
        JOIN af_template_category tc
        ON vtc.category_id = tc.category_id
        WHERE view_id = $1
        GROUP BY view_id
      ),
      creator_number_of_templates AS (
        SELECT
          creator_id,
          COUNT(*) AS number_of_templates
        FROM af_template_view
        GROUP BY creator_id
      )

      SELECT
        template.view_id,
        template.created_at,
        template.updated_at,
        template.name,
        template.description,
        template.about,
        template.view_url,
        (
          creator.creator_id,
          creator.name,
          creator.avatar_url,
          template_with_creator_account_link.account_links,
          creator_number_of_templates.number_of_templates
        )::template_creator_type AS "creator!: AFTemplateCreatorRow",
        template_with_category.categories AS "categories!: Vec<AFTemplateCategoryRow>",
        COALESCE(template_with_related_template.related_templates, '{}') AS "related_templates!: Vec<AFTemplateMinimalRow>",
        template.is_new_template,
        template.is_featured
      FROM af_template_view template
      JOIN af_template_creator creator
      USING (creator_id)
      JOIN template_with_creator_account_link
      ON template.view_id = template_with_creator_account_link.view_id
      LEFT OUTER JOIN template_with_related_template
      ON template.view_id = template_with_related_template.view_id
      JOIN template_with_category
      ON template.view_id = template_with_category.view_id
      LEFT OUTER JOIN creator_number_of_templates
      ON template.creator_id = creator_number_of_templates.creator_id
      WHERE template.view_id = $1

    "#,
    view_id
  )
  .fetch_one(executor)
  .await?;
  let view = view_row.into();
  Ok(view)
}

pub async fn select_templates<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  category_id: Option<Uuid>,
  is_featured: Option<bool>,
  is_new_template: Option<bool>,
  name_contains: Option<&str>,
  limit: Option<i64>,
) -> Result<Vec<TemplateMinimal>, AppError> {
  let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
    r#"
    WITH template_with_template_category AS (
      SELECT
        template_template_category.view_id,
        ARRAY_AGG((
          template_template_category.category_id,
          category.name,
          category.icon,
          category.bg_color
        )::template_category_minimal_type) AS categories
      FROM af_template_view_template_category template_template_category
      JOIN af_template_category category
      USING (category_id)
      JOIN af_template_view template
      USING (view_id)
      JOIN af_published_collab
      USING (view_id)
      WHERE TRUE
    "#,
  );
  if let Some(category_id) = category_id {
    query_builder.push(" AND template_template_category.category_id = ");
    query_builder.push_bind(category_id);
  };
  if let Some(is_featured) = is_featured {
    query_builder.push(" AND template.is_featured = ");
    query_builder.push_bind(is_featured);
  };
  if let Some(is_new_template) = is_new_template {
    query_builder.push(" AND template.is_new_template = ");
    query_builder.push_bind(is_new_template);
  };
  if let Some(name_contains) = name_contains {
    query_builder.push(" AND template.name ILIKE CONCAT('%', ");
    query_builder.push_bind(name_contains);
    query_builder.push(" , '%')");
  };
  query_builder.push(
    r#"
      GROUP BY template_template_category.view_id
    )

    SELECT
      template.view_id,
      template.created_at,
      template.updated_at,
      template.name,
      template.description,
      template.view_url,
      (
        template_creator.creator_id,
        template_creator.name,
        template_creator.avatar_url
      )::template_creator_minimal_type AS creator,
      tc.categories AS categories,
      template.is_new_template,
      template.is_featured
    FROM template_with_template_category tc
    JOIN af_template_view template
    USING (view_id)
    JOIN af_template_creator template_creator
    USING (creator_id)
    ORDER BY template.created_at DESC
    "#,
  );
  if let Some(limit) = limit {
    query_builder.push(" LIMIT ");
    query_builder.push_bind(limit);
  };
  let query = query_builder.build_query_as::<AFTemplateMinimalRow>();
  let template_rows: Vec<AFTemplateMinimalRow> = query.fetch_all(executor).await?;
  Ok(template_rows.into_iter().map(|row| row.into()).collect())
}

pub async fn select_template_homepage<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  per_count: i64,
) -> Result<Vec<TemplateGroup>, AppError> {
  let template_group_rows = sqlx::query_as!(
    AFTemplateGroupRow,
    r#"
      WITH recent_template AS (
        SELECT
          template_template_category.category_id,
          template_template_category.view_id,
          category.name,
          category.icon,
          category.bg_color,
          ROW_NUMBER() OVER (PARTITION BY template_template_category.category_id ORDER BY template.created_at DESC) AS recency
        FROM af_template_view_template_category template_template_category
        JOIN af_template_category category
        USING (category_id)
        JOIN af_template_view template
        USING (view_id)
        JOIN af_published_collab
        USING (view_id)
      ),
      template_group_by_category_and_view AS (
        SELECT
          category_id,
          view_id,
          ARRAY_AGG((
            category_id,
            name,
            icon,
            bg_color
          )::template_category_minimal_type) AS categories
          FROM recent_template
          WHERE recency <= $1
          GROUP BY category_id, view_id
      ),
      template_group_by_category_and_view_with_creator_and_template_details AS (
        SELECT
          template_group_by_category_and_view.category_id,
          (
            template.view_id,
            template.created_at,
            template.updated_at,
            template.name,
            template.description,
            template.view_url,
            (
              creator.creator_id,
              creator.name,
              creator.avatar_url
            )::template_creator_minimal_type,
            template_group_by_category_and_view.categories,
            template.is_new_template,
            template.is_featured
          )::template_minimal_type AS template
        FROM template_group_by_category_and_view
        JOIN af_template_view template
        USING (view_id)
        JOIN af_template_creator creator
        USING (creator_id)
      ),
      template_group_by_category AS (
        SELECT
          category_id,
          ARRAY_AGG(template) AS templates
        FROM template_group_by_category_and_view_with_creator_and_template_details
        GROUP BY category_id
      )
      SELECT
        (
          template_group_by_category.category_id,
          category.name,
          category.icon,
          category.bg_color
        )::template_category_minimal_type AS "category!: AFTemplateCategoryMinimalRow",
        templates AS "templates!: Vec<AFTemplateMinimalRow>"
        FROM template_group_by_category
        JOIN af_template_category category
        USING (category_id)
    "#,
    per_count,
  )
  .fetch_all(executor)
  .await?;
  Ok(
    template_group_rows
      .into_iter()
      .map(|row| row.into())
      .collect(),
  )
}

pub async fn delete_template_by_view_id<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  view_id: Uuid,
) -> Result<(), AppError> {
  sqlx::query!(
    r#"
    DELETE FROM af_template_view
    WHERE view_id = $1
    "#,
    view_id,
  )
  .execute(executor)
  .await?;
  Ok(())
}
