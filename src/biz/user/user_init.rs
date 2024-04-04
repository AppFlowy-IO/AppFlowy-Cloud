use crate::biz::collab::storage::CollabAccessControlStorage;
use app_error::AppError;
use database::collab::CollabStorage;
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
  workspace_id: &str,
  txn: &mut Transaction<'_, sqlx::Postgres>,
  templates: Vec<T>,
  collab_storage: &Arc<CollabAccessControlStorage>,
) -> anyhow::Result<(), AppError>
where
  T: WorkspaceTemplate + Send + Sync + 'static,
{
  let templates = WorkspaceTemplateBuilder::new(uid, workspace_id)
    .with_templates(templates)
    .build()
    .await?;

  debug!("create {} templates for user:{}", templates.len(), uid);
  for template in templates {
    let object_id = template.object_id;
    let encoded_collab_v1 = template
      .object_data
      .encode_to_bytes()
      .map_err(|err| AppError::Internal(anyhow::Error::from(err)))?;

    collab_storage
      .insert_or_update_collab_with_transaction(
        workspace_id,
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
