use std::path::Path;

use actix_multipart::form::bytes::Bytes;
use app_error::AppError;
use aws_sdk_s3::primitives::ByteStream;
use database::file::{s3_client_impl::AwsS3BucketClientImpl, BucketClient, ResponseBlob};
use database_entity::dto::UserImageAssetContent;
use uuid::Uuid;

fn user_image_asset_object_key(person_id: &Uuid, file_id: &str) -> String {
  format!("user-asset/image/{}/{}", person_id, file_id)
}

pub async fn upload_user_image_asset(
  client: AwsS3BucketClientImpl,
  image_content: &Bytes,
  person_id: &Uuid,
) -> Result<String, AppError> {
  let content_type = match &image_content.content_type {
    Some(content_type) if content_type.type_() == mime::IMAGE => Ok(content_type.to_string()),
    Some(content_type) => Err(AppError::InvalidContentType(format!(
      "{} is not a valid image type",
      content_type
    ))),
    None => Err(AppError::InvalidContentType(
      "Missing mime type for image upload".to_string(),
    )),
  }?;
  let file_name = image_content
    .file_name
    .as_ref()
    .ok_or(AppError::InvalidContentType(
      "Missing file name for image upload".to_string(),
    ))?
    .as_str();
  let extension = Path::new(&file_name)
    .extension()
    .ok_or(AppError::InvalidContentType(
      "Missing file extension for image upload".to_string(),
    ))?
    .to_str()
    .ok_or(AppError::InvalidContentType(
      "Invalid file extension for image upload".to_string(),
    ))?;
  let file_id = format!("{}.{}", Uuid::new_v4(), extension);

  let object_key = user_image_asset_object_key(person_id, &file_id);
  client
    .put_blob_with_content_type(
      &object_key,
      ByteStream::from(image_content.data.to_vec()),
      &content_type,
    )
    .await?;
  Ok(file_id.to_string())
}

pub async fn get_user_image_asset(
  client: AwsS3BucketClientImpl,
  person_id: &Uuid,
  file_id: String,
) -> Result<UserImageAssetContent, AppError> {
  let object_key = user_image_asset_object_key(person_id, &file_id);
  let resp = client.get_blob(&object_key).await?;
  let content_type = resp.content_type().ok_or(AppError::InvalidContentType(
    "Missing content type for avatar".to_string(),
  ))?;
  Ok(UserImageAssetContent {
    data: resp.to_blob(),
    content_type: content_type.to_string(),
  })
}
