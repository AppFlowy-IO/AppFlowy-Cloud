use crate::file::{BlobKey, BucketClient, BucketStorage, ResponseBlob};
use anyhow::anyhow;
use app_error::AppError;
use async_trait::async_trait;
use aws_sdk_s3::operation::delete_object::DeleteObjectOutput;

use std::ops::Deref;

use aws_sdk_s3::error::SdkError;

use aws_sdk_s3::operation::delete_objects::DeleteObjectsOutput;
use aws_sdk_s3::operation::get_object::GetObjectError;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::types::{CompletedMultipartUpload, CompletedPart, Delete, ObjectIdentifier};
use aws_sdk_s3::Client;
use database_entity::file_dto::{
  CompleteUploadRequest, CreateUploadRequest, CreateUploadResponse, UploadPartData,
  UploadPartResponse,
};

use tracing::{error, trace};

pub type S3BucketStorage = BucketStorage<AwsS3BucketClientImpl>;

impl S3BucketStorage {
  pub fn from_bucket_impl(client: AwsS3BucketClientImpl, pg_pool: sqlx::PgPool) -> Self {
    Self::new(client, pg_pool)
  }
}

pub struct AwsS3BucketClientImpl {
  client: Client,
  bucket: String,
}

impl AwsS3BucketClientImpl {
  pub fn new(client: Client, bucket: String) -> Self {
    debug_assert!(!bucket.is_empty());
    AwsS3BucketClientImpl { client, bucket }
  }

  async fn complete_upload_and_get_metadata(
    &self,
    object_key: &str,
    upload_id: &str,
    completed_multipart_upload: CompletedMultipartUpload,
  ) -> Result<(usize, String), AppError> {
    // Complete the multipart upload
    let _ = self
      .client
      .complete_multipart_upload()
      .bucket(&self.bucket)
      .key(object_key)
      .upload_id(upload_id)
      .multipart_upload(completed_multipart_upload)
      .send()
      .await
      .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

    // Retrieve the object metadata using head_object
    let head_object_result = self
      .client
      .head_object()
      .bucket(&self.bucket)
      .key(object_key)
      .send()
      .await
      .map_err(|e| AppError::Internal(anyhow::anyhow!(e)))?;

    let content_length = head_object_result
      .content_length()
      .ok_or_else(|| AppError::Unhandled("Content-Length not found".to_string()))?;
    let content_type = head_object_result
      .content_type()
      .map(|s| s.to_string())
      .unwrap_or_else(|| "application/octet-stream".to_string());

    Ok((content_length as usize, content_type))
  }
}

#[async_trait]
impl BucketClient for AwsS3BucketClientImpl {
  type ResponseData = S3ResponseData;

  async fn pub_blob<P>(&self, id: &P, content: &[u8]) -> Result<(), AppError>
  where
    P: BlobKey + Send,
  {
    let key = id.object_key();
    trace!(
      "Uploading object to S3 bucket:{}, key {}, len: {}",
      self.bucket,
      key,
      content.len()
    );
    let body = ByteStream::from(content.to_vec());
    self
      .client
      .put_object()
      .bucket(&self.bucket)
      .key(key)
      .body(body)
      .send()
      .await
      .map_err(|err| anyhow!("Failed to upload object to S3: {}", err))?;

    Ok(())
  }

  async fn delete_blob(&self, object_key: &str) -> Result<Self::ResponseData, AppError> {
    let output = self
      .client
      .delete_object()
      .bucket(&self.bucket)
      .key(object_key)
      .send()
      .await
      .map_err(|err| anyhow!("Failed to delete object to S3: {}", err))?;

    Ok(S3ResponseData::new(output))
  }

  async fn get_blob(&self, object_key: &str) -> Result<Self::ResponseData, AppError> {
    match self
      .client
      .get_object()
      .bucket(&self.bucket)
      .key(object_key)
      .send()
      .await
    {
      Ok(output) => match output.body.collect().await {
        Ok(body) => {
          let data = body.into_bytes().to_vec();
          Ok(S3ResponseData::new_with_data(data))
        },
        Err(err) => Err(AppError::from(anyhow!("Failed to collect body: {}", err))),
      },
      Err(SdkError::ServiceError(service_err)) => match service_err.err() {
        GetObjectError::NoSuchKey(_) => Err(AppError::RecordNotFound(format!(
          "blob not found for key:{object_key}"
        ))),
        _ => Err(AppError::from(anyhow!(
          "Failed to get object from S3: {:?}",
          service_err
        ))),
      },
      Err(err) => Err(AppError::from(anyhow!(
        "Failed to get object from S3: {}",
        err
      ))),
    }
  }

  /// Create a new upload session
  /// https://docs.aws.amazon.com/AmazonS3/latest/userguide/mpuoverview.html
  async fn create_upload(
    &self,
    key: impl BlobKey,
    req: CreateUploadRequest,
  ) -> Result<CreateUploadResponse, AppError> {
    let object_key = key.object_key();
    trace!(
      "Creating upload to S3 bucket:{}, key {}, request: {}",
      self.bucket,
      object_key,
      req
    );
    let multipart_upload_res = self
      .client
      .create_multipart_upload()
      .bucket(&self.bucket)
      .key(&object_key)
      .content_type(req.content_type)
      .send()
      .await
      .map_err(|err| anyhow!(format!("Failed to create upload: {:?}", err)))?;

    match multipart_upload_res.upload_id {
      None => Err(anyhow!("Failed to create upload: upload_id is None").into()),
      Some(upload_id) => Ok(CreateUploadResponse {
        file_id: req.file_id,
        upload_id,
      }),
    }
  }

  async fn upload_part(
    &self,
    key: &impl BlobKey,
    req: UploadPartData,
  ) -> Result<UploadPartResponse, AppError> {
    if req.body.is_empty() {
      return Err(AppError::InvalidRequest("body is empty".to_string()));
    }
    let object_key = key.object_key();
    trace!(
      "Uploading part to S3 bucket:{}, key {}, request: {}",
      self.bucket,
      object_key,
      req,
    );
    let body = ByteStream::from(req.body);
    let upload_part_res = self
      .client
      .upload_part()
      .bucket(&self.bucket)
      .key(&object_key)
      .upload_id(&req.upload_id)
      .part_number(req.part_number)
      .body(body)
      .send()
      .await
      .map_err(|err| anyhow!(format!("Failed to upload part: {:?}", err)))?;

    match upload_part_res.e_tag {
      None => Err(anyhow!("Failed to upload part: e_tag is None").into()),
      Some(e_tag) => Ok(UploadPartResponse {
        part_num: req.part_number,
        e_tag,
      }),
    }
  }

  /// Return the content length and content type of the uploaded object
  async fn complete_upload(
    &self,
    key: &impl BlobKey,
    req: CompleteUploadRequest,
  ) -> Result<(usize, String), AppError> {
    let object_key = key.object_key();
    trace!(
      "Completing upload to S3 bucket:{}, key {}, request: {}",
      self.bucket,
      object_key,
      req,
    );
    let parts = req
      .parts
      .into_iter()
      .map(|part| {
        CompletedPart::builder()
          .e_tag(part.e_tag)
          .part_number(part.part_number)
          .build()
      })
      .collect::<Vec<_>>();
    let completed_multipart_upload = CompletedMultipartUpload::builder()
      .set_parts(Some(parts))
      .build();

    self
      .complete_upload_and_get_metadata(&object_key, &req.upload_id, completed_multipart_upload)
      .await
  }

  async fn remove_dir(&self, parent_dir: &str) -> Result<(), AppError> {
    let mut continuation_token = None;
    loop {
      let list_objects = self
        .client
        .list_objects_v2()
        .bucket(&self.bucket)
        .prefix(parent_dir)
        .set_continuation_token(continuation_token.clone())
        .send()
        .await
        .map_err(|err| anyhow!("Failed to list object: {}", err))?;

      let mut objects_to_delete: Vec<ObjectIdentifier> = list_objects
        .contents
        .unwrap_or_default()
        .into_iter()
        .filter_map(|object| {
          object.key.and_then(|key| {
            ObjectIdentifier::builder()
              .key(key)
              .build()
              .map_err(|e| {
                error!("Error building ObjectIdentifier: {:?}", e);
                e
              })
              .ok()
          })
        })
        .collect();

      trace!(
        "objects_to_delete: {:?} at directory: {}",
        objects_to_delete.len(),
        parent_dir
      );

      // Step 2: Delete the listed objects in batches of 1000
      while !objects_to_delete.is_empty() {
        let batch = if objects_to_delete.len() > 1000 {
          objects_to_delete.split_off(1000)
        } else {
          Vec::new()
        };

        trace!(
          "Deleting {} objects: {:?}",
          parent_dir,
          objects_to_delete
            .iter()
            .map(|object| &object.key)
            .collect::<Vec<&String>>()
        );

        let delete = Delete::builder()
          .set_objects(Some(objects_to_delete))
          .build()
          .map_err(|e| {
            println!("Error building Delete: {:?}", e);
            e
          })
          .map_err(|err| anyhow!("Failed to build delete object: {}", err))?;

        let delete_objects_output: DeleteObjectsOutput = self
          .client
          .delete_objects()
          .bucket(&self.bucket)
          .delete(delete)
          .send()
          .await
          .map_err(|err| anyhow!("Failed to delete delete object: {}", err))?;

        if let Some(errors) = delete_objects_output.errors {
          for error in errors {
            println!("Error deleting object: {:?}", error);
          }
        }

        objects_to_delete = batch;
      }

      // is_truncated is true if there are more objects to list. If it's false, it means we have listed all objects in the directory
      match list_objects.is_truncated {
        None => break,
        Some(is_truncated) => {
          if !is_truncated {
            break;
          }
        },
      }

      continuation_token = list_objects.next_continuation_token;
    }

    Ok(())
  }
}

#[derive(Debug)]
pub struct S3ResponseData {
  data: Vec<u8>,
}

impl Deref for S3ResponseData {
  type Target = Vec<u8>;

  fn deref(&self) -> &Self::Target {
    &self.data
  }
}

impl ResponseBlob for S3ResponseData {
  fn to_blob(self) -> Vec<u8> {
    self.data
  }
}

impl S3ResponseData {
  pub fn new(_output: DeleteObjectOutput) -> Self {
    S3ResponseData { data: Vec::new() }
  }

  pub fn new_with_data(data: Vec<u8>) -> Self {
    S3ResponseData { data }
  }
}
