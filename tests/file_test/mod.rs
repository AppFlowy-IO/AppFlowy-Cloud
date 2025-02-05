use std::borrow::Cow;
use std::ops::Deref;

mod delete_dir_test;
mod multiple_part_test;
mod put_and_get;
mod usage;

use appflowy_cloud::application::get_aws_s3_client;
use appflowy_cloud::config::config::S3Setting;
use database::file::s3_client_impl::AwsS3BucketClientImpl;
use lazy_static::lazy_static;
use secrecy::Secret;
use tracing::warn;

lazy_static! {
  pub static ref LOCALHOST_MINIO_URL: Cow<'static, str> =
    get_env_var("LOCALHOST_MINIO_URL", "http://localhost:9000");
  pub static ref LOCALHOST_MINIO_ACCESS_KEY: Cow<'static, str> =
    get_env_var("LOCALHOST_MINIO_ACCESS_KEY", "minioadmin");
  pub static ref LOCALHOST_MINIO_SECRET_KEY: Cow<'static, str> =
    get_env_var("LOCALHOST_MINIO_SECRET_KEY", "minioadmin");
  pub static ref LOCALHOST_MINIO_BUCKET_NAME: Cow<'static, str> =
    get_env_var("LOCALHOST_MINIO_BUCKET_NAME", "appflowy");
}

pub struct TestBucket(pub AwsS3BucketClientImpl);

impl Deref for TestBucket {
  type Target = AwsS3BucketClientImpl;

  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl TestBucket {
  pub async fn new() -> Self {
    let setting = S3Setting {
      create_bucket: true,
      use_minio: true,
      minio_url: LOCALHOST_MINIO_URL.to_string(),
      access_key: LOCALHOST_MINIO_ACCESS_KEY.to_string(),
      secret_key: Secret::new(LOCALHOST_MINIO_SECRET_KEY.to_string()),
      bucket: LOCALHOST_MINIO_BUCKET_NAME.to_string(),
      region: "".to_string(),
      presigned_url_endpoint: None,
    };
    let client = AwsS3BucketClientImpl::new(
      get_aws_s3_client(&setting).await.unwrap(),
      setting.bucket.clone(),
      LOCALHOST_MINIO_URL.to_string(),
      setting.presigned_url_endpoint.clone(),
    );
    Self(client)
  }
}

fn get_env_var<'default>(key: &str, default: &'default str) -> Cow<'default, str> {
  dotenvy::dotenv().ok();
  match std::env::var(key) {
    Ok(value) => Cow::Owned(value),
    Err(_) => {
      warn!("could not read env var {}: using default: {}", key, default);
      Cow::Borrowed(default)
    },
  }
}
