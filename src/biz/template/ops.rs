use std::{
  collections::{HashMap, HashSet},
  ops::DerefMut,
  path::Path,
};

use actix_multipart::form::bytes::Bytes as MPBytes;
use anyhow::Context;
use app_error::ErrorCode;
use aws_sdk_s3::primitives::ByteStream;
use database::{
  file::{s3_client_impl::AwsS3BucketClientImpl, BucketClient, ResponseBlob},
  publish::{select_publish_info_for_view_ids, select_published_collab_info},
  template::*,
};
use database_entity::dto::{
  AccountLink, AvatarContent, PublishInfo, Template, TemplateCategory, TemplateCategoryType,
  TemplateCreator, TemplateGroupWithPublishInfo, TemplateHomePage, TemplateMinimalWithPublishInfo,
  TemplateWithPublishInfo,
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

pub async fn get_templates_with_publish_info(
  pg_pool: &PgPool,
  category_id: Option<Uuid>,
  is_featured: Option<bool>,
  is_new_template: Option<bool>,
  name_contains: Option<&str>,
) -> Result<Vec<TemplateMinimalWithPublishInfo>, AppResponseError> {
  let templates = select_templates(
    pg_pool,
    category_id,
    is_featured,
    is_new_template,
    name_contains,
    None,
  )
  .await?;
  let view_ids = templates.iter().map(|t| t.view_id).collect::<Vec<Uuid>>();
  let publish_info_for_views = select_publish_info_for_view_ids(pg_pool, &view_ids).await?;
  let mut publish_info_map = publish_info_for_views
    .into_iter()
    .map(|info| (info.view_id, info))
    .collect::<HashMap<Uuid, PublishInfo>>();
  let templates_with_publish_info: Vec<TemplateMinimalWithPublishInfo> = templates
    .into_iter()
    .filter_map(|template| {
      publish_info_map
        .remove(&template.view_id)
        .map(|publish_info| TemplateMinimalWithPublishInfo {
          template,
          publish_info,
        })
    })
    .collect();
  if templates_with_publish_info.len() != view_ids.len() {
    return Err(AppResponseError::new(
      ErrorCode::Internal,
      "one or more templates does not have a publish info",
    ));
  }
  Ok(templates_with_publish_info)
}

pub async fn get_template_with_publish_info(
  pg_pool: &PgPool,
  view_id: Uuid,
) -> Result<TemplateWithPublishInfo, AppResponseError> {
  let template = select_template_view_by_id(pg_pool, view_id).await?;
  let publish_info = select_published_collab_info(pg_pool, &view_id).await?;
  let template_with_publish_info = TemplateWithPublishInfo {
    template,
    publish_info,
  };
  Ok(template_with_publish_info)
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
  let template_groups_view_ids = template_groups
    .iter()
    .flat_map(|group| group.templates.iter().map(|t| t.view_id))
    .collect::<HashSet<Uuid>>();
  let feature_templates_view_ids = featured_templates
    .iter()
    .map(|t| t.view_id)
    .collect::<HashSet<Uuid>>();
  let new_templates_view_ids = new_templates
    .iter()
    .map(|t| t.view_id)
    .collect::<HashSet<Uuid>>();
  let mut all_view_ids: HashSet<Uuid> = HashSet::new();
  all_view_ids.extend(&template_groups_view_ids);
  all_view_ids.extend(&feature_templates_view_ids);
  all_view_ids.extend(&new_templates_view_ids);
  let all_view_ids: Vec<Uuid> = all_view_ids.into_iter().collect();

  let publish_info_for_views: Vec<PublishInfo> =
    select_publish_info_for_view_ids(pg_pool, &all_view_ids).await?;
  let publish_info_map = publish_info_for_views
    .into_iter()
    .map(|info| (info.view_id, info))
    .collect::<HashMap<Uuid, PublishInfo>>();

  let template_groups_with_publish_info = template_groups
    .into_iter()
    .map(|group| {
      let templates_with_publish_info = group
        .templates
        .into_iter()
        .filter_map(|template| {
          publish_info_map.get(&template.view_id).map(|publish_info| {
            TemplateMinimalWithPublishInfo {
              template,
              publish_info: publish_info.clone(),
            }
          })
        })
        .collect();
      TemplateGroupWithPublishInfo {
        category: group.category.clone(),
        templates: templates_with_publish_info,
      }
    })
    .collect();

  let featured_templates_with_publish_info = featured_templates
    .into_iter()
    .filter_map(|template| {
      publish_info_map
        .get(&template.view_id)
        .map(|publish_info| TemplateMinimalWithPublishInfo {
          template,
          publish_info: publish_info.clone(),
        })
    })
    .collect();

  let new_templates_with_publish_info = new_templates
    .into_iter()
    .filter_map(|template| {
      publish_info_map
        .get(&template.view_id)
        .map(|publish_info| TemplateMinimalWithPublishInfo {
          template,
          publish_info: publish_info.clone(),
        })
    })
    .collect();

  let homepage = TemplateHomePage {
    template_groups: template_groups_with_publish_info,
    featured_templates: featured_templates_with_publish_info,
    new_templates: new_templates_with_publish_info,
  };
  Ok(homepage)
}

fn avatar_object_key(file_id: &str) -> String {
  format!("template-center/avatar/{}", file_id)
}

pub async fn get_avatar(
  client: AwsS3BucketClientImpl,
  file_id: String,
) -> Result<AvatarContent, AppResponseError> {
  let object_key = avatar_object_key(&file_id);
  let resp = client.get_blob(&object_key).await?;
  let content_type = resp.content_type().ok_or(AppResponseError::new(
    ErrorCode::InvalidContentType,
    "Missing content type for avatar".to_string(),
  ))?;
  Ok(AvatarContent {
    data: resp.to_blob(),
    content_type: content_type.to_string(),
  })
}

pub async fn upload_avatar(
  client: AwsS3BucketClientImpl,
  avatar: &MPBytes,
) -> Result<String, AppResponseError> {
  let content_type = match &avatar.content_type {
    Some(content_type) if content_type.type_() == mime::IMAGE => Ok(content_type.to_string()),
    Some(content_type) => Err(AppResponseError::new(
      ErrorCode::InvalidContentType,
      format!("Invalid mime type for avatar upload: {}", content_type),
    )),
    None => Err(AppResponseError::new(
      ErrorCode::InvalidContentType,
      "Missing mime type for avatar upload",
    )),
  }?;
  let file_name = avatar
    .file_name
    .as_ref()
    .ok_or(AppResponseError::new(
      ErrorCode::InvalidContentType,
      "Missing file name for avatar upload",
    ))?
    .as_str();
  let extension = Path::new(&file_name)
    .extension()
    .ok_or(AppResponseError::new(
      ErrorCode::InvalidContentType,
      "Missing file extension for avatar upload",
    ))?
    .to_str()
    .ok_or(AppResponseError::new(
      ErrorCode::InvalidContentType,
      "Invalid file extension for avatar upload",
    ))?;
  let file_id = format!("{}.{}", Uuid::new_v4(), extension);

  let object_key = avatar_object_key(&file_id);
  client
    .put_blob_with_content_type(
      &object_key,
      ByteStream::from(avatar.data.to_vec()),
      &content_type,
    )
    .await?;
  Ok(file_id.to_string())
}
