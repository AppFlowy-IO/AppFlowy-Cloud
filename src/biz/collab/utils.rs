use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use collab::core::collab::DataSource;
use collab::preclude::Collab;
use collab_database::database::DatabaseBody;
use collab_database::entity::FieldType;
use collab_database::fields::type_option_cell_reader;
use collab_database::fields::type_option_cell_writer;
use collab_database::fields::Field;
use collab_database::fields::TypeOptionCellReader;
use collab_database::fields::TypeOptionCellWriter;
use collab_database::fields::TypeOptionData;
use collab_database::fields::TypeOptions;
use collab_database::rows::Cell;
use collab_database::rows::RowDetail;
use collab_database::template::entity::CELL_DATA;
use collab_database::template::timestamp_parse::TimestampCellData;
use collab_database::workspace_database::NoPersistenceDatabaseCollabService;
use collab_entity::CollabType;
use collab_entity::EncodedCollab;
use collab_folder::CollabOrigin;
use database::collab::CollabStorage;
use database::collab::GetCollabOrigin;
use database_entity::dto::QueryCollab;
use database_entity::dto::QueryCollabParams;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

pub fn get_row_details_serde(
  row_detail: RowDetail,
  field_by_id_name_uniq: &HashMap<String, Field>,
  type_option_reader_by_id: &HashMap<String, Box<dyn TypeOptionCellReader>>,
) -> HashMap<String, HashMap<String, serde_json::Value>> {
  let mut cells = row_detail.row.cells;
  let mut row_details_serde: HashMap<String, HashMap<String, serde_json::Value>> =
    HashMap::with_capacity(cells.len());
  for (field_id, field) in field_by_id_name_uniq {
    let cell: Cell = match cells.remove(field_id) {
      Some(cell) => cell.clone(),
      None => {
        let field_type = FieldType::from(field.field_type);
        match field_type {
          FieldType::CreatedTime => {
            TimestampCellData::new(Some(row_detail.row.created_at)).to_cell(field_type)
          },
          FieldType::LastEditedTime => {
            TimestampCellData::new(Some(row_detail.row.modified_at)).to_cell(field_type)
          },
          _ => Cell::new(),
        }
      },
    };
    let cell_value = match type_option_reader_by_id.get(&field.id) {
      Some(tor) => tor.json_cell(&cell),
      None => {
        tracing::error!("Failed to get type option reader by id: {}", field.id);
        serde_json::Value::Null
      },
    };
    row_details_serde.insert(
      field.name.clone(),
      HashMap::from([(CELL_DATA.to_string(), cell_value)]),
    );
  }

  row_details_serde
}

/// create a map of field name to field
/// if the field name is repeated, it will be appended with the field id,
pub fn field_by_name_uniq(mut fields: Vec<Field>) -> HashMap<String, Field> {
  fields.sort_by_key(|a| a.id.clone());
  let mut uniq_name_set: HashSet<String> = HashSet::with_capacity(fields.len());
  let mut field_by_name: HashMap<String, Field> = HashMap::with_capacity(fields.len());

  for field in fields {
    // if the name already exists, append the field id to the name
    let name = if uniq_name_set.contains(&field.name) {
      format!("{}-{}", field.name, field.id)
    } else {
      field.name.clone()
    };
    uniq_name_set.insert(name.clone());
    field_by_name.insert(name, field);
  }
  field_by_name
}

/// create a map of field id to field name, and ensure that the field name is unique.
/// if the field name is repeated, it will be appended with the field id,
/// under practical usage circumstances, no other collision should occur
pub fn field_by_id_name_uniq(mut fields: Vec<Field>) -> HashMap<String, Field> {
  fields.sort_by_key(|a| a.id.clone());
  let mut uniq_name_set: HashSet<String> = HashSet::with_capacity(fields.len());
  let mut field_by_id: HashMap<String, Field> = HashMap::with_capacity(fields.len());

  for mut field in fields {
    // if the name already exists, append the field id to the name
    if uniq_name_set.contains(&field.name) {
      let new_name = format!("{}-{}", field.name, field.id);
      field.name.clone_from(&new_name);
    }
    uniq_name_set.insert(field.name.clone());
    field_by_id.insert(field.id.clone(), field);
  }
  field_by_id
}

/// create a map type option writer by field id
pub fn type_option_writer_by_id(
  fields: &[Field],
) -> HashMap<String, Box<dyn TypeOptionCellWriter>> {
  let mut type_option_reader_by_id: HashMap<String, Box<dyn TypeOptionCellWriter>> =
    HashMap::with_capacity(fields.len());
  for field in fields {
    let field_id: String = field.id.clone();
    let type_option_reader: Box<dyn TypeOptionCellWriter> = {
      let field_type: &FieldType = &FieldType::from(field.field_type);
      let type_option_data: TypeOptionData = match field.get_any_type_option(field_type.type_id()) {
        Some(tod) => tod.clone(),
        None => HashMap::new(),
      };
      type_option_cell_writer(type_option_data, field_type)
    };
    type_option_reader_by_id.insert(field_id, type_option_reader);
  }
  type_option_reader_by_id
}

/// create a map type option reader by field id
pub fn type_option_reader_by_id(
  fields: &[Field],
) -> HashMap<String, Box<dyn TypeOptionCellReader>> {
  let mut type_option_reader_by_id: HashMap<String, Box<dyn TypeOptionCellReader>> =
    HashMap::with_capacity(fields.len());
  for field in fields {
    let field_id: String = field.id.clone();
    let type_option_reader: Box<dyn TypeOptionCellReader> = {
      let field_type: &FieldType = &FieldType::from(field.field_type);
      let type_option_data: TypeOptionData = match field.get_any_type_option(field_type.type_id()) {
        Some(tod) => tod.clone(),
        None => HashMap::new(),
      };
      type_option_cell_reader(type_option_data, field_type)
    };
    type_option_reader_by_id.insert(field_id, type_option_reader);
  }
  type_option_reader_by_id
}

pub fn type_options_serde(
  type_options: &TypeOptions,
  field_type: &FieldType,
) -> HashMap<String, serde_json::Value> {
  let type_option = match type_options.get(&field_type.type_id()) {
    Some(type_option) => type_option,
    None => return HashMap::new(),
  };

  let mut result = HashMap::with_capacity(type_option.len());
  for (key, value) in type_option {
    match field_type {
      FieldType::SingleSelect | FieldType::MultiSelect | FieldType::Media => {
        // Certain type option are stored as stringified JSON
        // We need to parse them back to JSON
        // e.g. "{ \"key\": \"value\" }" -> { "key": "value" }
        if let yrs::Any::String(arc_str) = value {
          if let Ok(serde_value) = serde_json::from_str::<serde_json::Value>(arc_str) {
            result.insert(key.clone(), serde_value);
          }
        }
      },
      _ => {
        result.insert(key.clone(), serde_json::to_value(value).unwrap_or_default());
      },
    }
  }

  result
}

pub async fn get_database_body(
  collab_storage: &CollabAccessControlStorage,
  workspace_uuid_str: &str,
  database_uuid_str: &str,
) -> Result<(Collab, DatabaseBody), AppError> {
  let db_collab = get_latest_collab(
    collab_storage,
    GetCollabOrigin::Server,
    workspace_uuid_str,
    database_uuid_str,
    CollabType::Database,
  )
  .await?;
  let db_body = DatabaseBody::from_collab(
    &db_collab,
    Arc::new(NoPersistenceDatabaseCollabService),
    None,
  )
  .ok_or_else(|| {
    AppError::Internal(anyhow::anyhow!(
      "Failed to create database body from collab, db_collab_id: {}",
      database_uuid_str,
    ))
  })?;
  Ok((db_collab, db_body))
}

pub async fn get_latest_collab_encoded(
  collab_storage: &CollabAccessControlStorage,
  collab_origin: GetCollabOrigin,
  workspace_id: &str,
  oid: &str,
  collab_type: CollabType,
) -> Result<EncodedCollab, AppError> {
  collab_storage
    .get_encode_collab(
      collab_origin,
      QueryCollabParams {
        workspace_id: workspace_id.to_string(),
        inner: QueryCollab {
          object_id: oid.to_string(),
          collab_type,
        },
      },
      true,
    )
    .await
}

pub async fn get_latest_collab(
  storage: &CollabAccessControlStorage,
  origin: GetCollabOrigin,
  workspace_id: &str,
  oid: &str,
  collab_type: CollabType,
) -> Result<Collab, AppError> {
  let ec = get_latest_collab_encoded(storage, origin, workspace_id, oid, collab_type).await?;
  let collab: Collab = Collab::new_with_source(CollabOrigin::Server, oid, ec.into(), vec![], false)
    .map_err(|e| {
      AppError::Internal(anyhow::anyhow!(
        "Failed to create collab from encoded collab: {:?}",
        e
      ))
    })?;
  Ok(collab)
}

pub async fn collab_to_bin(collab: Collab, collab_type: CollabType) -> Result<Vec<u8>, AppError> {
  tokio::task::spawn_blocking(move || {
    let bin = collab
      .encode_collab_v1(|collab| collab_type.validate_require_data(collab))
      .map_err(|e| AppError::Unhandled(e.to_string()))?
      .encode_to_bytes()?;
    Ok(bin)
  })
  .await?
}

pub fn collab_from_doc_state(doc_state: Vec<u8>, object_id: &str) -> Result<Collab, AppError> {
  let collab = Collab::new_with_source(
    CollabOrigin::Server,
    object_id,
    DataSource::DocStateV1(doc_state),
    vec![],
    false,
  )
  .map_err(|e| AppError::Unhandled(e.to_string()))?;
  Ok(collab)
}
