use anyhow::Result;
use appflowy_worker::import_worker::task_queue::ImportTask;
use appflowy_worker::import_worker::unzip::unzip_async;
use async_zip::base::read::stream::ZipFileReader;
use futures::io::Cursor;
use redis::streams::{StreamReadOptions, StreamReadReply};
use redis::{Commands, RedisResult, Value};
use serde_json::{from_str, json};
use std::path::PathBuf;
use tokio::fs;

#[tokio::test]
async fn create_task_test() {
  let mut redis_client = redis_client().await;
  let stream_name = "import_notion_task_stream";
  let consumer_group = "import_notion_task_group";
  let consumer_name = "appflowy_worker_notion_importer";
  let workspace_id = uuid::Uuid::new_v4().to_string();
  let user_uuid = uuid::Uuid::new_v4().to_string();

  let _: RedisResult<()> = redis_client.xgroup_create_mkstream(stream_name, consumer_group, "0");

  // 1. insert a task
  let task = json!({
      "notion": {
         "uid": 1,
         "user_uuid": user_uuid,
         "workspace_id": workspace_id,
         "s3_key": workspace_id,
         "file_type": "zip",
         "host": "http::localhost",
      }
  });

  let _: () = redis_client
    .xadd(stream_name, "*", &[("task", task.to_string())])
    .unwrap();
  tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

  // 2. consume a task
  let options = StreamReadOptions::default()
    .group(consumer_group, consumer_name)
    .count(3);

  let tasks: StreamReadReply = redis_client
    .xread_options(&[stream_name], &[">"], &options)
    .unwrap();

  assert!(!tasks.keys.is_empty());

  for stream_key in tasks.keys {
    for stream_id in stream_key.ids {
      let task_str = match stream_id.map.get("task") {
        Some(value) => match value {
          Value::Data(data) => String::from_utf8_lossy(data).to_string(),
          _ => panic!("Task field is not a string"),
        },
        None => continue,
      };

      let task = from_str::<ImportTask>(&task_str).unwrap();
      println!("{:?}", task);

      let _: () = redis_client
        .xack(stream_name, consumer_group, &[stream_id.id.clone()])
        .unwrap();
    }
  }
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
