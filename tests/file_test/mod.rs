use std::borrow::Cow;

mod put_and_get;
mod usage;
use lazy_static::lazy_static;
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

pub struct TestBucket(pub s3::Bucket);

impl TestBucket {
  pub async fn new() -> Self {
    let region = s3::Region::Custom {
      region: "".to_owned(),
      endpoint: LOCALHOST_MINIO_URL.to_string(),
    };

    let cred = s3::creds::Credentials {
      access_key: Some(LOCALHOST_MINIO_ACCESS_KEY.to_string()),
      secret_key: Some(LOCALHOST_MINIO_SECRET_KEY.to_string()),
      security_token: None,
      session_token: None,
      expiration: None,
    };

    match s3::Bucket::create_with_path_style(
      &LOCALHOST_MINIO_BUCKET_NAME,
      region.clone(),
      cred.clone(),
      s3::BucketConfiguration::default(),
    )
    .await
    {
      Ok(_) => {},
      Err(e) => match e {
        s3::error::S3Error::HttpFailWithBody(409, _) => {},
        _ => panic!("could not create bucket: {}", e),
      },
    }

    Self(
      s3::Bucket::new(&LOCALHOST_MINIO_BUCKET_NAME, region.clone(), cred.clone())
        .unwrap()
        .with_path_style(),
    )
  }

  pub async fn get_object(&self, workspace_id: &str, file_id: &str) -> Option<bytes::Bytes> {
    let object_key = format!("{}/{}", workspace_id, file_id);
    match self.0.get_object(&object_key).await {
      Ok(resp) => {
        assert!(resp.status_code() == 200);
        Some(resp.bytes().to_owned())
      },
      Err(err) => match err {
        s3::error::S3Error::HttpFailWithBody(404, _) => None,
        _ => panic!("could not get object: {}", err),
      },
    }
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
