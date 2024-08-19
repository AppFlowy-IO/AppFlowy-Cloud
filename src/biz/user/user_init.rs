use std::sync::Arc;

use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use collab::core::origin::CollabOrigin;
use collab::preclude::{ArrayPrelim, Collab, Map};
use collab_entity::define::WORKSPACE_DATABASES;
use collab_entity::CollabType;
use collab_user::core::UserAwareness;
use database::collab::CollabStorage;
use database::pg_row::AFWorkspaceRow;
use database_entity::dto::CollabParams;
use sqlx::Transaction;
use tracing::{debug, error, instrument, trace};
use uuid::Uuid;
use workspace_template::{WorkspaceTemplate, WorkspaceTemplateBuilder};

/// This function generates templates for a workspace and stores them in the database.
/// Each template is stored as an individual collaborative object.
#[instrument(level = "debug", skip_all, err)]
pub async fn initialize_workspace_for_user<T>(
  uid: i64,
  user_uuid: &Uuid,
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

    match create_user_awareness(&uid, user_uuid, &workspace_id, collab_storage, txn).await {
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
          embeddings: None,
        },
        txn,
      )
      .await?;
  }
  Ok(())
}

async fn create_user_awareness(
  uid: &i64,
  user_uuid: &Uuid,
  workspace_id: &str,
  storage: &Arc<CollabAccessControlStorage>,
  txn: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<String, AppError> {
  let object_id = user_awareness_object_id(user_uuid, workspace_id).to_string();
  let collab_type = CollabType::UserAwareness;
  let collab = Collab::new_with_origin(CollabOrigin::Empty, object_id.clone(), vec![], false);

  // TODO(nathan): Maybe using hardcode encoded collab
  let user_awareness = UserAwareness::open(collab, None);
  let encode_collab = user_awareness
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
        embeddings: None,
      },
      txn,
    )
    .await?;
  Ok(object_id)
}

async fn create_workspace_database_collab(
  workspace_id: &str,
  uid: &i64,
  object_id: &str,
  storage: &Arc<CollabAccessControlStorage>,
  txn: &mut Transaction<'_, sqlx::Postgres>,
) -> Result<(), AppError> {
  let collab_type = CollabType::WorkspaceDatabase;
  let mut collab = Collab::new_with_origin(CollabOrigin::Empty, object_id, vec![], false);
  {
    let mut txn = collab.transact_mut();
    collab
      .data
      .insert(&mut txn, WORKSPACE_DATABASES, ArrayPrelim::default());
  };

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
        embeddings: None,
      },
      txn,
    )
    .await?;

  Ok(())
}

pub fn user_awareness_object_id(user_uuid: &Uuid, workspace_id: &str) -> Uuid {
  Uuid::new_v5(
    user_uuid,
    format!("user_awareness:{}", workspace_id).as_bytes(),
  )
}
