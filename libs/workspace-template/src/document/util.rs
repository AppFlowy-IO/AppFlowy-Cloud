use crate::database::database_collab::create_database_collab;
use crate::document::parser::JsonToDocumentParser;
use crate::{TemplateData, TemplateObjectId};
use collab::core::collab::CollabOptions;
use collab::core::origin::CollabOrigin;
use collab::preclude::Collab;
use collab_database::entity::CreateDatabaseParams;
use collab_document::document::Document;
use collab_entity::CollabType;

pub async fn create_document_from_json(
  object_id: String,
  json_str: &str,
) -> anyhow::Result<TemplateData> {
  // 1. read the getting started document from the assets
  let document_data = JsonToDocumentParser::json_str_to_document(json_str)?;

  // 2. create a new document with the getting started data
  let data = tokio::task::spawn_blocking(move || {
    let options = CollabOptions::new(object_id.clone());
    let collab = Collab::new_with_options(CollabOrigin::Empty, options)?;
    let document = Document::create_with_data(collab, document_data)?;
    let encoded_collab = document.encode_collab()?;

    Ok::<_, anyhow::Error>(TemplateData {
      template_id: TemplateObjectId::Document(object_id),
      collab_type: CollabType::Document,
      encoded_collab,
    })
  })
  .await??;

  Ok(data)
}

/// Create a series of database templates from the given JSON String
///
/// Notes: The output contains DatabaseCollab, DatabaseRowCollab
pub async fn create_database_from_params(
  object_id: String,
  create_database_params: CreateDatabaseParams,
) -> anyhow::Result<Vec<TemplateData>> {
  let database_id = create_database_params.database_id.clone();
  let encoded_database = create_database_collab(create_database_params).await?;

  let encoded_database_collab = encoded_database
    .encoded_database_collab
    .encoded_collab
    .clone();

  // 1. create the new database collab
  let database_template_data = TemplateData {
    template_id: TemplateObjectId::Database {
      object_id,
      database_id: database_id.clone(),
    },
    collab_type: CollabType::Database,
    encoded_collab: encoded_database_collab,
  };

  // 2. create the new database row collabs
  let database_row_template_data =
    encoded_database
      .encoded_row_collabs
      .into_iter()
      .map(|encoded_row_collab| TemplateData {
        template_id: TemplateObjectId::DatabaseRow(encoded_row_collab.object_id),
        collab_type: CollabType::DatabaseRow,
        encoded_collab: encoded_row_collab.encoded_collab,
      });

  let mut template_data = vec![database_template_data];
  template_data.extend(database_row_template_data);

  Ok(template_data)
}
