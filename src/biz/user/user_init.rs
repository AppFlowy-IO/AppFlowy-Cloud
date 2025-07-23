use app_error::AppError;
use collab::core::collab::{default_client_id, CollabOptions};
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_database::workspace_database::WorkspaceDatabase;
use collab_entity::CollabType;
use collab_folder::{Folder, FolderData, Workspace};
use collab_user::core::UserAwareness;
use database::collab::CollabStore;
use database::pg_row::AFWorkspaceRow;
use database_entity::dto::CollabParams;
use sqlx::Transaction;
use std::sync::Arc;
use tracing::{error, instrument, trace};
use uuid::Uuid;
use workspace_template::{TemplateObjectId, WorkspaceTemplate, WorkspaceTemplateBuilder};

/// This function generates templates for a workspace and stores them in the database.
/// Each template is stored as an individual collaborative object.
#[instrument(level = "debug", skip_all, err)]
pub async fn initialize_workspace_for_user<T>(
  uid: i64,
  user_uuid: &Uuid,
  row: &AFWorkspaceRow,
  txn: &mut Transaction<'_, sqlx::Postgres>,
  templates: Vec<T>,
  collab_storage: &Arc<dyn CollabStore>,
) -> anyhow::Result<(), AppError>
where
  T: WorkspaceTemplate + Send + Sync + 'static,
{
  let workspace_id = row.workspace_id;
  let templates = WorkspaceTemplateBuilder::new(uid, &workspace_id)
    .with_templates(templates)
    .build()
    .await?;

  let mut database_records = vec![];
  let mut collab_params = Vec::with_capacity(templates.len());
  for template in templates {
    let template_id = template.template_id;
    let (view_id, object_id) = match &template_id {
      TemplateObjectId::Document(oid) => (oid.to_string(), oid.to_string()),
      TemplateObjectId::Folder(oid) => (oid.to_string(), oid.to_string()),
      TemplateObjectId::DatabaseRow(oid) => (oid.to_string(), oid.to_string()),
      TemplateObjectId::Database {
        object_id,
        database_id,
      } => (object_id.clone(), database_id.clone()),
    };
    let object_id = Uuid::parse_str(&object_id)?;
    let object_type = template.collab_type;
    let encoded_collab_v1 = template
      .encoded_collab
      .encode_to_bytes()
      .map_err(|err| AppError::Internal(anyhow::Error::from(err)))?;
    collab_params.push(CollabParams {
      object_id,
      encoded_collab_v1: encoded_collab_v1.into(),
      collab_type: object_type,
      updated_at: None,
    });

    // push the database record
    if object_type == CollabType::Database {
      if let TemplateObjectId::Database {
        object_id: _,
        database_id,
      } = &template_id
      {
        database_records.push((view_id, database_id.clone()));
      }
    }
  }

  collab_storage
    .batch_insert_new_collab(workspace_id, &uid, collab_params)
    .await?;

  // Create a workspace database object for given user
  // The database_storage_id is auto-generated when the workspace is created. So, it should be available
  if let Some(&database_storage_id) = row.database_storage_id.as_ref() {
    create_workspace_database_collab(
      workspace_id,
      &uid,
      database_storage_id,
      collab_storage,
      txn,
      database_records,
    )
    .await?;

    match create_user_awareness(&uid, user_uuid, workspace_id, collab_storage, txn).await {
      Ok(object_id) => trace!("User awareness created successfully: {}", object_id),
      Err(err) => {
        error!(
          "Failed to create user awareness for workspace: {}, {}",
          workspace_id, err
        );
      },
    }
  } else {
    return Err(AppError::Internal(anyhow::anyhow!(
      "Workspace database object id is missing"
    )));
  }

  Ok(())
}

pub(crate) async fn create_user_awareness(
  uid: &i64,
  user_uuid: &Uuid,
  workspace_id: Uuid,
  storage: &Arc<dyn CollabStore>,
  txn: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<Uuid, AppError> {
  let object_id = user_awareness_object_id(user_uuid, &workspace_id);
  let collab_type = CollabType::UserAwareness;
  let options = CollabOptions::new(object_id.to_string(), default_client_id());
  let collab = Collab::new_with_options(CollabOrigin::Empty, options)
    .map_err(|e| AppError::Internal(e.into()))?;

  // TODO(nathan): Maybe using hardcode encoded collab
  let user_awareness = UserAwareness::create(collab, None)?;
  let encode_collab = user_awareness
    .encode_collab_v1(|collab| collab_type.validate_require_data(collab))
    .map_err(|err| AppError::Internal(err.into()))?;
  let encoded_collab_v1 = encode_collab
    .encode_to_bytes()
    .map_err(|err| AppError::Internal(anyhow::Error::from(err)))?;

  storage
    .upsert_new_collab_with_transaction(
      workspace_id,
      uid,
      CollabParams {
        object_id,
        encoded_collab_v1: encoded_collab_v1.into(),
        collab_type,
        updated_at: None,
      },
      txn,
      "create user awareness",
    )
    .await?;
  Ok(object_id)
}

pub(crate) async fn create_workspace_collab(
  uid: i64,
  workspace_id: Uuid,
  name: &str,
  storage: &Arc<dyn CollabStore>,
  txn: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<(), AppError> {
  let workspace = Workspace::new(workspace_id.to_string(), name.to_string(), uid);
  let folder_data = FolderData::new(uid, workspace);

  let options = CollabOptions::new(workspace_id.to_string(), default_client_id());
  let collab = Collab::new_with_options(CollabOrigin::Empty, options)
    .map_err(|e| AppError::Internal(e.into()))?;
  let folder = Folder::create(collab, None, folder_data);
  let encode_collab = folder
    .encode_collab()
    .map_err(|err| AppError::Internal(err.into()))?;

  let encoded_collab_v1 = encode_collab
    .encode_to_bytes()
    .map_err(|err| AppError::Internal(anyhow::Error::from(err)))?;

  storage
    .upsert_new_collab_with_transaction(
      workspace_id,
      &uid,
      CollabParams {
        object_id: workspace_id,
        encoded_collab_v1: encoded_collab_v1.into(),
        collab_type: CollabType::Folder,
        updated_at: None,
      },
      txn,
      "create workspace collab",
    )
    .await?;
  Ok(())
}

pub(crate) async fn create_workspace_database_collab(
  workspace_id: Uuid,
  uid: &i64,
  object_id: Uuid,
  storage: &Arc<dyn CollabStore>,
  txn: &mut Transaction<'_, sqlx::Postgres>,
  initial_database_records: Vec<(String, String)>,
) -> Result<(), AppError> {
  let collab_type = CollabType::WorkspaceDatabase;
  let options = CollabOptions::new(object_id.to_string(), default_client_id());
  let collab = Collab::new_with_options(CollabOrigin::Empty, options)
    .map_err(|e| AppError::Internal(e.into()))?;
  let mut workspace_database = WorkspaceDatabase::create(collab);
  for (object_id, database_id) in initial_database_records {
    workspace_database.add_database(&database_id, vec![object_id]);
  }
  let encode_collab = workspace_database
    .encode_collab_v1()
    .map_err(|err| AppError::Internal(err.into()))?;

  let encoded_collab_v1 = encode_collab
    .encode_to_bytes()
    .map_err(|err| AppError::Internal(anyhow::Error::from(err)))?;

  storage
    .upsert_new_collab_with_transaction(
      workspace_id,
      uid,
      CollabParams {
        object_id,
        encoded_collab_v1: encoded_collab_v1.into(),
        collab_type,
        updated_at: None,
      },
      txn,
      "create database collab",
    )
    .await?;

  Ok(())
}

pub fn user_awareness_object_id(user_uuid: &Uuid, workspace_id: &Uuid) -> Uuid {
  Uuid::new_v5(
    user_uuid,
    format!("user_awareness:{}", workspace_id).as_bytes(),
  )
}
