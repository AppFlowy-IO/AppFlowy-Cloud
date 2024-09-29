use anyhow::Result;
use appflowy_worker::import_worker::unzip::unzip_async;
use async_zip::base::read::stream::ZipFileReader;
use futures::io::Cursor;
use std::path::PathBuf;
use tokio::fs;

#[tokio::test]
async fn create_task_test() {
  let redis_client = redis_client().await;
}

#[tokio::test]
async fn test_unzip_async() -> Result<()> {
  let zip_path = PathBuf::from("tests/asset/project&task.zip");
  let zip_data = fs::read(&zip_path).await?;
  let cursor = Cursor::new(zip_data);
  let output_dir = std::env::temp_dir().join("test_unzip_output");
  fs::create_dir_all(&output_dir).await?;
  let zip_reader = ZipFileReader::new(cursor);
  let extract_file = unzip_async(zip_reader, output_dir.clone()).await?;
  assert!(
    extract_file.exists(),
    "The first extracted file should exist"
  );

  let file_name = extract_file.file_name().unwrap().to_str().unwrap();
  assert_eq!(file_name, "project&task");

  Ok(())
}

pub async fn redis_client() -> redis::Client {
  let redis_uri = "redis://localhost:6379";
  redis::Client::open(redis_uri).unwrap()
}
