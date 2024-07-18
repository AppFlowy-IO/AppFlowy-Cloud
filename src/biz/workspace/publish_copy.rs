use app_error::AppError;
use collab::core::collab::DataSource;
use collab_entity::{CollabType, EncodedCollab};
use collab_folder::CollabOrigin;
use sqlx::PgPool;
use uuid::Uuid;

pub async fn copy_published_collab_to_workspace(
  pg_pool: &PgPool,
  publish_view_id: Uuid,
  dest_workspace_id: Uuid,
  collab_type: CollabType,
) -> Result<(), AppError> {
  let copier = PublishCollabCopier::new(dest_workspace_id, pg_pool.clone());
  copier.deep_copy(publish_view_id, collab_type).await?;
  Ok(())
}

pub struct PublishCollabCopier {
  dest_workspace_id: Uuid,
  pg_pool: PgPool,
  // TODO: maybe need to add target view_id or some folder structure
}

impl PublishCollabCopier {
  pub fn new(dest_workspace_id: Uuid, pg_pool: PgPool) -> Self {
    Self {
      dest_workspace_id,
      pg_pool,
    }
  }

  pub async fn deep_copy(
    &self,
    publish_view_id: Uuid,
    collab_type: CollabType,
  ) -> Result<(), AppError> {
    let mut txn = self.pg_pool.begin().await?;
    self
      .deep_copy_txn(&mut txn, publish_view_id, collab_type)
      .await?;
    txn.commit().await?;
    Ok(())
  }

  pub async fn deep_copy_txn(
    &self,
    txn: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    publish_view_id: Uuid,
    collab_type: CollabType,
  ) -> Result<(), AppError> {
    let mut _tx = self.pg_pool.begin().await?;

    // get the type and data of the view_id
    let collab_data =
      match database::publish::select_published_collab_blob_for_view_id(txn, &publish_view_id)
        .await?
      {
        Some(bin_data) => bin_data,
        None => {
          tracing::warn!(
            "No published collab data found for view_id: {}",
            publish_view_id
          );
          return Ok(());
        },
      };

    let encoded_collab = EncodedCollab::decode_from_bytes(&collab_data)?;

    match collab_type {
      CollabType::Document => {
        let _collab_doc = collab_document::document::Document::from_doc_state(
          CollabOrigin::Empty,
          DataSource::DocStateV1(encoded_collab.doc_state.to_vec()), // only use v1
          "",
          vec![],
        )
        .unwrap();
      },
      CollabType::Database => todo!(),
      CollabType::DatabaseRow => todo!(),
      t => tracing::warn!("collab type not supported: {:?}", t),
    }

    _ = self.dest_workspace_id;
    todo!();
  }
}
