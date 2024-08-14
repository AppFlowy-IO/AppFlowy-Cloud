use app_error::AppError;
use database_entity::dto::{
  AccountLink, Template, TemplateCategory, TemplateCategoryType, TemplateCreator, TemplateGroup,
  TemplateMinimal,
};
use sqlx::{Executor, Postgres, QueryBuilder};
use uuid::Uuid;

use crate::pg_row::{
  AFTemplateCategoryMinimalRow, AFTemplateCategoryRow, AFTemplateCategoryTypeColumn,
  AFTemplateCreatorMinimalColumn, AFTemplateCreatorRow, AFTemplateGroupRow, AFTemplateMinimalRow,
  AFTemplateRow, AccountLinkColumn,
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
      priority = $7
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
          SET name = $2, avatar_url = $3
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
      view_id = $1,
      name = $2,
      description = $3,
      about = $4,
      view_url = $5,
      creator_id = $6,
      is_new_template = $7,
      is_featured = $8
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
      WITH template_view_creator_account_link AS (
        SELECT
          view_id,
          tv.creator_id,
          COALESCE(
            ARRAY_AGG((al.link_type, al.url)::account_link_type) FILTER (WHERE link_type IS NOT NULL),
            '{}'
          ) AS account_links
        FROM af_template_view tv
        JOIN af_template_creator tc
        ON tv.creator_id = tc.creator_id
        LEFT OUTER JOIN af_template_creator_account_link al
        ON tc.creator_id = al.creator_id
        WHERE view_id = $1
        GROUP BY (view_id, tv.creator_id)
      ),
      related_template_view_template_category AS (
        SELECT
          rtv.related_view_id,
          ARRAY_AGG(
            (
              tcg.category_id,
              tcg.name,
              tcg.icon,
              tcg.bg_color
            )::template_category_minimal_type
          ) AS categories
        FROM af_related_template_view rtv
        JOIN af_template_view tv
        ON rtv.view_id = tv.view_id
        JOIN af_template_view_template_category tvc
        ON tv.view_id = tvc.view_id
        JOIN af_template_category tcg
        ON tvc.category_id = tcg.category_id
        WHERE rtv.view_id = $1
        GROUP BY rtv.related_view_id
      ),
      template_view_related_template AS (
        SELECT
          rtv.view_id,
          ARRAY_AGG(
            (
              rtv.related_view_id,
              tv.name,
              tv.description,
              tv.view_url,
              (
                tc.creator_id,
                tc.name,
                tc.avatar_url
              )::template_creator_minimal_type,
              rtvtc.categories,
              tv.is_new_template,
              tv.is_featured
            )::template_minimal_type
          ) AS related_templates
        FROM af_related_template_view rtv
        JOIN af_template_view tv
        ON rtv.related_view_id = tv.view_id
        JOIN af_template_creator tc
        ON tv.creator_id = tc.creator_id
        JOIN related_template_view_template_category rtvtc
        ON rtv.related_view_id = rtvtc.related_view_id
        WHERE rtv.view_id = $1
        GROUP BY rtv.view_id
      ),
      template_view_template_category AS (
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
      )

      SELECT
        tv.view_id,
        tv.created_at,
        tv.updated_at,
        tv.name,
        tv.description,
        tv.about,
        tv.view_url,
        (
          atc.creator_id,
          atc.name,
          atc.avatar_url,
          al.account_links
        )::template_creator_type AS "creator!: AFTemplateCreatorRow",
        tc.categories AS "categories!: Vec<AFTemplateCategoryRow>",
        COALESCE(rtv.related_templates, '{}') AS "related_templates!: Vec<AFTemplateMinimalRow>",
        tv.is_new_template,
        tv.is_featured
      FROM af_template_view tv
      JOIN af_template_creator atc
      ON tv.creator_id = atc.creator_id
      JOIN template_view_creator_account_link al
      ON tv.view_id = al.view_id
      LEFT OUTER JOIN template_view_related_template rtv
      ON tv.view_id = rtv.view_id
      JOIN template_view_template_category tc
      ON tv.view_id = tc.view_id
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
) -> Result<Vec<TemplateMinimal>, AppError> {
  let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
    r#"
    WITH template_view_template_category AS (
      SELECT
        vtc.view_id,
        ARRAY_AGG((
          vtc.category_id,
          tc.name,
          tc.icon,
          tc.bg_color
        )::template_category_minimal_type) AS categories
      FROM af_template_view_template_category vtc
      JOIN af_template_category tc
      ON vtc.category_id = tc.category_id
      JOIN af_template_view tv
      ON vtc.view_id = tv.view_id
      WHERE TRUE
    "#,
  );
  if let Some(category_id) = category_id {
    query_builder.push(" AND vtc.category_id = ");
    query_builder.push_bind(category_id);
  };
  if let Some(is_featured) = is_featured {
    query_builder.push(" AND tv.is_featured = ");
    query_builder.push_bind(is_featured);
  };
  if let Some(is_new_template) = is_new_template {
    query_builder.push(" AND tv.is_new_template = ");
    query_builder.push_bind(is_new_template);
  };
  if let Some(name_contains) = name_contains {
    query_builder.push(" AND tv.name ILIKE CONCAT('%', ");
    query_builder.push_bind(name_contains);
    query_builder.push(" , '%')");
  };
  query_builder.push(
    r#"
      GROUP BY vtc.view_id
    )

    SELECT
      tv.view_id,
      tv.name,
      tv.description,
      tv.view_url,
      (
        atc.creator_id,
        atc.name,
        atc.avatar_url
      )::template_creator_minimal_type AS creator,
      tc.categories AS categories,
      tv.is_new_template,
      tv.is_featured
    FROM template_view_template_category tc
    JOIN af_template_view tv
    ON tc.view_id = tv.view_id
    JOIN af_template_creator atc
    ON tv.creator_id = atc.creator_id
    "#,
  );
  let query = query_builder.build_query_as::<AFTemplateMinimalRow>();
  let template_rows: Vec<AFTemplateMinimalRow> = query.fetch_all(executor).await?;
  Ok(template_rows.into_iter().map(|row| row.into()).collect())
}

pub async fn select_template_homepage<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
) -> Result<Vec<TemplateGroup>, AppError> {
  let template_group_rows = sqlx::query_as!(
    AFTemplateGroupRow,
    r#"
      WITH grouped_template_view_template_categories AS (
        SELECT
          tvtc.category_id,
          tvtc.view_id,
          ARRAY_AGG((
            tvtc.category_id,
            tc.name,
            tc.bg_color,
            tc.icon
          )::template_category_minimal_type) AS categories
        FROM af_template_view_template_category tvtc
        JOIN af_template_category tc
        ON tvtc.category_id = tc.category_id
        GROUP BY tvtc.category_id, tvtc.view_id
      ),
      grouped_template_view AS (
        SELECT
          gtvtc.category_id,
          (
            tv.view_id,
            tv.name,
            tv.description,
            tv.view_url,
            (
              tc.creator_id,
              tc.name,
              tc.avatar_url
            )::template_creator_minimal_type,
            gtvtc.categories,
            tv.is_new_template,
            tv.is_featured
          )::template_minimal_type AS template
        FROM grouped_template_view_template_categories gtvtc
        JOIN af_template_view tv
        ON gtvtc.view_id = tv.view_id
        JOIN af_template_creator tc
        ON tv.creator_id = tc.creator_id
      ),
      grouped_template_view_template_with_category_id AS (
        SELECT
          category_id,
          ARRAY_AGG(template) AS templates
        FROM grouped_template_view
        GROUP BY category_id
      )
      SELECT
        (
          gtv.category_id,
          tcg.name,
          tcg.bg_color,
          tcg.icon
        )::template_category_minimal_type AS "category!: AFTemplateCategoryMinimalRow",
        templates AS "templates!: Vec<AFTemplateMinimalRow>"
        FROM grouped_template_view_template_with_category_id gtv
        JOIN af_template_category tcg
        ON gtv.category_id = tcg.category_id
    "#,
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
