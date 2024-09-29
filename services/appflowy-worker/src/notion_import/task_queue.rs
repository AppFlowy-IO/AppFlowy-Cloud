use crate::error::{ImportError, WorkerError};
use crate::notion_import::unzip::unzip_async;
use crate::s3_client::{S3Client, S3StreamResponse};
use anyhow::anyhow;
use async_zip::base::read::stream::ZipFileReader;
use aws_sdk_s3::primitives::ByteStream;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab_entity::CollabType;
use collab_folder::Folder;
use collab_importer::imported_collab::ImportType;
use collab_importer::notion::page::CollabResource;
use collab_importer::notion::NotionImporter;
use collab_importer::util::FileId;
use database::collab::select_blob_from_af_collab;
use futures::stream::FuturesUnordered;
use futures::{stream, StreamExt};
use redis::aio::ConnectionManager;
use redis::streams::{StreamReadOptions, StreamReadReply};
use redis::{AsyncCommands, Value};
use serde::{Deserialize, Serialize};
use serde_json::from_str;
use sqlx::PgPool;
use std::env::temp_dir;
use std::path::{Path, PathBuf};

use std::time::Duration;
use tokio::fs;

use tokio::task::spawn_local;
use tokio::time::interval;
use tracing::{error, info, trace};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportTask {
  uid: i64,
  user_uuid: String,
  workspace_id: String,
  s3_key: String,
  file_type: ImportFileType,
  host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportFileType {
  Zip,
}

pub async fn run_notion_importer(
  mut redis_client: ConnectionManager,
  s3_client: S3Client,
  pg_pool: PgPool,
) -> Result<(), ImportError> {
  info!("Starting notion importer worker");

  let stream_name = "import_notion_task_stream";
  let consumer_group = "import_notion_task_group";
  let consumer_name = "appflowy_worker_notion_importer";

  let options = StreamReadOptions::default()
    .group(consumer_group, consumer_name)
    .count(3);
  let mut interval = interval(Duration::from_secs(30));
  interval.tick().await;

  loop {
    interval.tick().await;
    let tasks: StreamReadReply = match redis_client
      .xread_options(&[stream_name], &[">"], &options)
      .await
    {
      Ok(tasks) => tasks,
      Err(err) => {
        error!("Failed to read tasks from Redis stream: {:?}", err);
        continue;
      },
    };

    let mut task_handlers = FuturesUnordered::new();
    for stream_key in tasks.keys {
      // For each stream key, iterate through the stream entries
      for stream_id in stream_key.ids {
        let task_str = match stream_id.map.get("task") {
          Some(value) => match value {
            Value::Data(data) => String::from_utf8_lossy(data).to_string(),
            _ => {
              error!("Unexpected value type for task field: {:?}", value);
              continue;
            },
          },
          None => {
            error!("Task field not found in Redis stream entry");
            continue;
          },
        };

        match from_str::<ImportTask>(&task_str) {
          Ok(import_task) => {
            let entry_id = stream_id.id.clone();
            let mut cloned_redis_client = redis_client.clone();
            let cloned_s3_client = s3_client.clone();
            let pg_pool = pg_pool.clone();
            task_handlers.push(spawn_local(async move {
              process_task(import_task, &cloned_s3_client, &pg_pool).await?;
              let _: () = cloned_redis_client
                .xack(stream_name, consumer_group, &[entry_id])
                .await
                .map_err(|e| {
                  error!("Failed to acknowledge task: {:?}", e);
                  ImportError::Internal(e.into())
                })?;
              Ok::<_, ImportError>(())
            }));
          },
          Err(err) => {
            error!("Failed to deserialize task: {:?}", err);
          },
        }
      }
    }

    while let Some(result) = task_handlers.next().await {
      match result {
        Ok(Ok(())) => {
          trace!("Task completed successfully");
        },
        Ok(Err(e)) => {
          error!("Task failed: {:?}", e);
        },
        Err(e) => {
          error!("Runtime error: {:?}", e);
        },
      }
    }
  }
}

async fn process_task(
  import_task: ImportTask,
  s3_client: &S3Client,
  _pg_pool: &PgPool,
) -> Result<(), ImportError> {
  trace!("Processing task: {:?}", import_task);

  // 1. unzip file to temp dir
  let unzip_file = download_zip_file(&import_task, s3_client).await?;

  // 2. import zip
  // let result = process_unzip_file(&import_task, &unzip_file, pg_pool, s3_client).await;

  // 3. delete zip file regardless of success or failure
  if let Err(err) = fs::remove_file(unzip_file).await {
    error!("Failed to delete unzip file: {:?}", err);
  }

  // 4. notify import result
  // notify_user(&import_task, result).await?;

  // 5. remove file from S3
  if let Err(err) = s3_client.delete_blob(import_task.s3_key.as_str()).await {
    error!("Failed to delete zip file from S3: {:?}", err);
  }

  Ok(())
}

async fn download_zip_file(
  import_task: &ImportTask,
  s3_client: &S3Client,
) -> Result<PathBuf, ImportError> {
  let S3StreamResponse {
    stream,
    content_type: _,
  } = s3_client
    .get_blob(import_task.s3_key.as_str())
    .await
    .map_err(|err| ImportError::Internal(err.into()))?;

  let zip_reader = ZipFileReader::new(stream);
  let temp_dir = temp_dir();
  let unzip_file = unzip_async(zip_reader, temp_dir)
    .await
    .map_err(|err| ImportError::Internal(err.into()))?;
  Ok(unzip_file)
}

async fn process_unzip_file(
  import_task: &ImportTask,
  unzip_file: &PathBuf,
  pg_pool: &PgPool,
  s3_client: &S3Client,
) -> Result<(), ImportError> {
  let notion_importer = NotionImporter::new(
    unzip_file,
    import_task.workspace_id.clone(),
    import_task.host.clone(),
  )
  .map_err(ImportError::ImportCollabError)?;

  let imported = notion_importer
    .import()
    .await
    .map_err(ImportError::ImportCollabError)?;
  let nested_views = imported.build_nested_views(import_task.uid).await;

  // 1. Open the workspace folder
  let folder_collab =
    get_encode_collab_from_bytes(&imported.workspace_id, &CollabType::Folder, pg_pool).await?;
  let mut folder = Folder::from_collab_doc_state(
    import_task.uid,
    CollabOrigin::Server,
    folder_collab.into(),
    &imported.workspace_id,
    vec![],
  )
  .map_err(|err| ImportError::CannotOpenWorkspace(err.to_string()))?;

  let mut resources = vec![];
  // 2. Start a transaction to insert all collabs to the database
  let transaction = pg_pool.begin().await.map_err(|err| {
    ImportError::Internal(anyhow!(
      "Failed to start transaction when importing data: {:?}",
      err
    ))
  })?;

  let mut stream = imported.into_collab_stream().await;
  while let Some(imported_collab) = stream.next().await {
    trace!("Imported collab: {}", imported_collab);
    resources.push(imported_collab.resource);
    match imported_collab.import_type {
      ImportType::Database => {},
      ImportType::Document => {},
    }
  }

  // 3. Insert collabs' views into the folder
  folder.insert_nested_views(nested_views.into_inner());

  // 4. write folder collab to disk
  let folder_collab = folder
    .encode_collab_v1(|collab| CollabType::Folder.validate_require_data(collab))
    .map_err(|err| ImportError::Internal(err.into()))?;

  transaction.commit().await.map_err(|err| {
    ImportError::Internal(anyhow!(
      "Failed to commit transaction when importing data: {:?}",
      err
    ))
  })?;

  // 5. after inserting all collabs, upload all files to S3
  batch_upload_files_to_s3(&import_task.workspace_id, s3_client, resources)
    .await
    .map_err(|err| ImportError::Internal(anyhow!("Failed to upload files to S3: {:?}", err)))?;

  Ok(())
}

async fn notify_user(
  import_task: &ImportTask,
  result: Result<(), ImportError>,
) -> Result<(), WorkerError> {
  // send email
  Ok(())
}

pub async fn batch_upload_files_to_s3(
  workspace_id: &str,
  client: &S3Client,
  collab_resources: Vec<CollabResource>,
) -> Result<(), anyhow::Error> {
  // Flatten the collab_resources into an iterator of (workspace_id, object_id, file_path)
  let file_tasks = collab_resources
    .into_iter()
    .flat_map(|resource| {
      let object_id = resource.object_id;
      resource
        .files
        .into_iter()
        .map(move |file| (object_id.clone(), file))
    })
    .collect::<Vec<(String, String)>>();

  // Create a stream of upload tasks
  let upload_stream = stream::iter(file_tasks.into_iter().map(
    |(object_id, file_path)| async move {
      match upload_file_to_s3(client, workspace_id, &object_id, &file_path).await {
        Ok(_) => {
          trace!("Successfully uploaded: {}", file_path);
          Ok(())
        },
        Err(e) => {
          error!("Failed to upload {}: {:?}", file_path, e);
          Err(e)
        },
      }
    },
  ))
  .buffer_unordered(5);
  let results: Vec<_> = upload_stream.collect().await;
  let errors: Vec<_> = results.into_iter().filter_map(Result::err).collect();
  if errors.is_empty() {
    Ok(())
  } else {
    Err(anyhow!("Some uploads failed: {:?}", errors))
  }
}

async fn upload_file_to_s3(
  client: &S3Client,
  workspace_id: &str,
  object_id: &str,
  file_path: &str,
) -> Result<(), anyhow::Error> {
  let path = Path::new(file_path);
  if !path.exists() {
    return Err(anyhow!("File does not exist: {:?}", path));
  }
  let file_id = FileId::from_path(&path.to_path_buf()).await?;
  let mime_type = mime_guess::from_path(file_path).first_or_octet_stream();
  let object_key = format!("{}/{}/{}", workspace_id, object_id, file_id);
  let byte_stream = ByteStream::from_path(path).await?;
  client
    .put_blob(&object_key, byte_stream, Some(mime_type.as_ref()))
    .await?;
  Ok(())
}

async fn get_encode_collab_from_bytes(
  object_id: &str,
  collab_type: &CollabType,
  pg_pool: &PgPool,
) -> Result<EncodedCollab, ImportError> {
  let bytes = select_blob_from_af_collab(pg_pool, collab_type, object_id)
    .await
    .map_err(|err| ImportError::Internal(err.into()))?;
  tokio::task::spawn_blocking(move || match EncodedCollab::decode_from_bytes(&bytes) {
    Ok(encoded_collab) => Ok(encoded_collab),
    Err(err) => Err(ImportError::Internal(anyhow!(
      "Failed to decode collab from bytes: {:?}",
      err
    ))),
  })
  .await
  .map_err(|err| ImportError::Internal(err.into()))?
}
