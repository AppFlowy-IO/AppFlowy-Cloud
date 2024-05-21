use app_error::AppError;

use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use collab::core::origin::CollabOrigin;
use collab::preclude::{Any, Collab, MapPrelim};
use collab_entity::define::WORKSPACE_DATABASES;
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database::pg_row::AFWorkspaceRow;
use database_entity::dto::CollabParams;
use sqlx::Transaction;
use std::sync::Arc;
use tracing::{debug, instrument};
use workspace_template::{WorkspaceTemplate, WorkspaceTemplateBuilder};

/// This function generates templates for a workspace and stores them in the database.
/// Each template is stored as an individual collaborative object.
#[instrument(level = "debug", skip_all, err)]
pub async fn initialize_workspace_for_user<T>(
  uid: i64,
  row: &AFWorkspaceRow,
  txn: &mut Transaction<'_, sqlx::Postgres>,
  templates: Vec<T>,
  collab_storage: &Arc<CollabAccessControlStorage>,
) -> anyhow::Result<(), AppError>
where
  T: WorkspaceTemplate + Send + Sync + 'static,
{
  let workspace_id = row.workspace_id.to_string();
  let templates = WorkspaceTemplateBuilder::new(uid, &workspace_id)
    .with_templates(templates)
    .build()
    .await?;

  // Create a workspace database object for given user
  // The database_storage_id is auto-generated when the workspace is created. So, it should be available
  if let Some(database_storage_id) = row.database_storage_id.as_ref() {
    let workspace_database_object_id = database_storage_id.to_string();
    create_workspace_database_collab(
      &workspace_id,
      &uid,
      &workspace_database_object_id,
      collab_storage,
      txn,
    )
    .await?;
  } else {
    return Err(AppError::Internal(anyhow::anyhow!(
      "Workspace database object id is missing"
    )));
  }

  debug!("create {} templates for user:{}", templates.len(), uid);
  for template in templates {
    let object_id = template.object_id;
    let encoded_collab_v1 = template
      .object_data
      .encode_to_bytes()
      .map_err(|err| AppError::Internal(anyhow::Error::from(err)))?;

    collab_storage
      .insert_new_collab_with_transaction(
        &workspace_id,
        &uid,
        CollabParams {
          object_id,
          encoded_collab_v1,
          collab_type: template.object_type,
        },
        txn,
      )
      .await?;
  }
  Ok(())
}

async fn create_workspace_database_collab(
  workspace_id: &str,
  uid: &i64,
  object_id: &str,
  storage: &Arc<CollabAccessControlStorage>,
  txn: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<(), AppError> {
  let collab_type = CollabType::WorkspaceDatabase;
  let collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], false);
  let _ = collab.with_origin_transact_mut(|txn| {
    collab.create_array_with_txn::<MapPrelim<Any>>(txn, WORKSPACE_DATABASES, vec![]);
    Ok::<(), AppError>(())
  });

  let encode_collab = collab
    .encode_collab_v1(|collab| collab_type.validate_require_data(collab))
    .map_err(AppError::Internal)?;

  let encoded_collab_v1 = encode_collab
    .encode_to_bytes()
    .map_err(|err| AppError::Internal(anyhow::Error::from(err)))?;

  storage
    .insert_new_collab_with_transaction(
      workspace_id,
      uid,
      CollabParams {
        object_id: object_id.to_string(),
        encoded_collab_v1,
        collab_type,
      },
      txn,
    )
    .await?;

  Ok(())
}
