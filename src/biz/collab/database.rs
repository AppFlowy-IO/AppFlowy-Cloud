use std::sync::Arc;

use super::utils::batch_get_latest_collab_encoded;
use app_error::AppError;
use async_trait::async_trait;
use collab::lock::RwLock;
use collab_database::database_trait::{DatabaseCollabReader, EncodeCollabByOid};
use collab_database::rows::{DatabaseRow, RowId};
use collab_database::{
  database::{gen_database_group_id, gen_field_id},
  entity::FieldType,
  error::DatabaseError,
  fields::{
    date_type_option::DateTypeOption, default_field_settings_for_fields,
    select_type_option::SingleSelectTypeOption, Field, TypeOptionData,
  },
  views::{
    BoardLayoutSetting, CalendarLayoutSetting, DatabaseLayout, FieldSettingsByFieldIdMap, Group,
    GroupSetting, GroupSettingMap, LayoutSettings,
  },
};
use collab_entity::{CollabType, EncodedCollab};
use dashmap::DashMap;
use database::collab::{CollabStore, GetCollabOrigin};
use uuid::Uuid;
use yrs::block::ClientID;

pub struct LinkedViewDependencies {
  pub layout_settings: LayoutSettings,
  pub field_settings: FieldSettingsByFieldIdMap,
  pub group_settings: Vec<GroupSettingMap>,
  pub deps_fields: Vec<Field>,
}

pub fn resolve_dependencies_when_create_database_linked_view(
  database_layout: DatabaseLayout,
  fields: &[Field],
) -> Result<LinkedViewDependencies, AppError> {
  match database_layout {
    DatabaseLayout::Grid => resolve_grid_dependencies(fields),
    DatabaseLayout::Board => resolve_board_dependencies(fields),
    DatabaseLayout::Calendar => resolve_calendar_dependencies(fields),
  }
}

fn resolve_grid_dependencies(fields: &[Field]) -> Result<LinkedViewDependencies, AppError> {
  Ok(LinkedViewDependencies {
    layout_settings: LayoutSettings::default(),
    field_settings: default_field_settings_for_fields(fields, DatabaseLayout::Grid),
    group_settings: vec![],
    deps_fields: vec![],
  })
}

fn resolve_board_dependencies(
  original_fields: &[Field],
) -> Result<LinkedViewDependencies, AppError> {
  let database_layout = DatabaseLayout::Board;
  let (group_field, all_fields, deps_fields) = match original_fields
    .iter()
    .find(|f| FieldType::from(f.field_type).can_be_group())
  {
    Some(field) => (field.clone(), original_fields.to_vec(), vec![]),
    None => {
      let card_status_field = create_card_status_field();
      let mut fields = original_fields.to_vec();
      fields.push(card_status_field.clone());
      (card_status_field.clone(), fields, vec![card_status_field])
    },
  };
  let field_settings = default_field_settings_for_fields(&all_fields, database_layout);
  let group_ids = match FieldType::from(group_field.field_type) {
    FieldType::SingleSelect => {
      let mut group_ids = vec![group_field.id.clone()];
      let single_select_type_option_ids = single_select_type_option_ids_from_field(&group_field)?;
      group_ids.extend(single_select_type_option_ids);
      Ok(group_ids)
    },
    FieldType::Checkbox => Ok(vec!["Yes".to_string(), "No".to_string()]),
    field_type => Err(AppError::Internal(anyhow::anyhow!(format!(
      "invalid dep field type({}) for board layout",
      field_type
    )))),
  }?;

  let groups = group_ids.iter().map(|id| Group::new(id.clone())).collect();
  let group_settings: Vec<GroupSettingMap> = vec![GroupSetting {
    id: gen_database_group_id(),
    field_id: group_field.id.clone(),
    field_type: group_field.field_type,
    groups,
    content: Default::default(),
  }
  .into()];

  let mut layout_settings = LayoutSettings::default();
  layout_settings.insert(database_layout, BoardLayoutSetting::new().into());
  Ok(LinkedViewDependencies {
    layout_settings,
    field_settings,
    group_settings,
    deps_fields,
  })
}

fn single_select_type_option_ids_from_field(field: &Field) -> Result<Vec<String>, AppError> {
  let type_option_data: Option<&TypeOptionData> =
    field.type_options.get(&FieldType::SingleSelect.to_string());
  match type_option_data {
    Some(type_option_data) => {
      let single_select_type_option = SingleSelectTypeOption::from(type_option_data.to_owned());
      let single_select_type_option_ids: Vec<String> = single_select_type_option
        .options
        .iter()
        .map(|option| option.id.clone())
        .collect();
      Ok(single_select_type_option_ids)
    },
    None => Err(AppError::Internal(anyhow::anyhow!(
      "invalid field for single select type options",
    ))),
  }
}

fn resolve_calendar_dependencies(fields: &[Field]) -> Result<LinkedViewDependencies, AppError> {
  let database_layout = DatabaseLayout::Calendar;
  let (date_time_field, all_fields, deps_fields) = match fields
    .iter()
    .find(|f| FieldType::from(f.field_type) == FieldType::DateTime)
  {
    Some(field) => {
      let all_fields = fields.to_vec();
      (field.clone(), all_fields, vec![])
    },
    None => {
      let date_field = create_date_field();
      let mut all_fields = fields.to_vec();
      all_fields.push(date_field.clone());
      (date_field.clone(), all_fields, vec![date_field])
    },
  };
  let field_settings = default_field_settings_for_fields(&all_fields, database_layout);
  let mut layout_settings = LayoutSettings::default();
  let layout_setting = CalendarLayoutSetting::new(date_time_field.id.clone()).into();
  layout_settings.insert(database_layout, layout_setting);
  Ok(LinkedViewDependencies {
    layout_settings,
    field_settings,
    group_settings: vec![],
    deps_fields,
  })
}

fn create_date_field() -> Field {
  let field_type = FieldType::DateTime;
  let default_date_type_option = DateTypeOption::default();
  let field_id = gen_field_id();
  Field::new(field_id, "Date".to_string(), field_type.into(), false)
    .with_type_option_data(field_type, default_date_type_option.into())
}

fn create_card_status_field() -> Field {
  let field_type = FieldType::SingleSelect;
  let default_select_type_option = SingleSelectTypeOption::default();
  let field_id = gen_field_id();
  Field::new(field_id, "Status".to_string(), field_type.into(), false)
    .with_type_option_data(field_type, default_select_type_option.into())
}

#[derive(Clone)]
pub struct PostgresDatabaseCollabService {
  pub workspace_id: Uuid,
  pub collab_storage: Arc<dyn CollabStore>,
  pub client_id: ClientID,
  cache: Arc<DashMap<RowId, Arc<RwLock<DatabaseRow>>>>,
}

impl PostgresDatabaseCollabService {
  pub fn new(
    workspace_id: Uuid,
    collab_storage: Arc<dyn CollabStore>,
    client_id: ClientID,
  ) -> Self {
    Self {
      workspace_id,
      collab_storage,
      client_id,
      cache: Arc::new(DashMap::new()),
    }
  }
}

#[async_trait]
impl DatabaseCollabReader for PostgresDatabaseCollabService {
  async fn reader_client_id(&self) -> ClientID {
    self.client_id
  }

  async fn reader_get_collab(
    &self,
    object_id: &str,
    collab_type: CollabType,
  ) -> Result<EncodedCollab, DatabaseError> {
    let object_id = Uuid::parse_str(object_id)?;
    let collab_data = self
      .collab_storage
      .get_full_encode_collab(
        GetCollabOrigin::Server,
        &self.workspace_id,
        &object_id,
        collab_type,
      )
      .await
      .map(|v| v.encoded_collab)
      .map_err(|err| DatabaseError::Internal(err.into()))?;

    Ok(collab_data)
  }

  async fn reader_batch_get_collabs(
    &self,
    object_ids: Vec<String>,
    collab_type: CollabType,
  ) -> Result<EncodeCollabByOid, DatabaseError> {
    let mut object_uuids = Vec::with_capacity(object_ids.len());
    for object_id in object_ids {
      object_uuids.push(Uuid::parse_str(&object_id)?);
    }
    let encoded_collabs = batch_get_latest_collab_encoded(
      &self.collab_storage,
      GetCollabOrigin::Server,
      self.workspace_id,
      &object_uuids,
      collab_type,
    )
    .await
    .unwrap()
    .into_iter()
    .map(|(k, v)| (k.to_string(), v))
    .collect();
    Ok(encoded_collabs)
  }

  fn database_row_cache(&self) -> Option<Arc<DashMap<RowId, Arc<RwLock<DatabaseRow>>>>> {
    Some(self.cache.clone())
  }
}

#[cfg(test)]
mod tests {
  use collab_database::{
    fields::select_type_option::{SelectOption, SelectOptionColor},
    views::LayoutSetting,
  };

  use super::*;

  #[test]
  fn test_resolve_dependencies_when_create_database_linked_view_grid() {
    let database_layout = DatabaseLayout::Grid;
    let fields: Vec<Field> = vec![];
    let dependencies =
      resolve_dependencies_when_create_database_linked_view(database_layout, &fields).unwrap();
    assert!(dependencies.deps_fields.is_empty());
    let fields: Vec<Field> = vec![
      Field::from_field_type("name", FieldType::RichText, true),
      Field::from_field_type("description", FieldType::RichText, false),
    ];
    let dependencies =
      resolve_dependencies_when_create_database_linked_view(database_layout, &fields).unwrap();
    assert!(dependencies.deps_fields.is_empty());
  }

  #[test]
  fn test_resolve_dependencies_when_create_database_linked_view_board() {
    let database_layout = DatabaseLayout::Board;
    let fields: Vec<Field> = vec![];
    let dependencies =
      resolve_dependencies_when_create_database_linked_view(database_layout, &fields).unwrap();
    assert_eq!(dependencies.deps_fields.len(), 1);
    let deps_field = dependencies.deps_fields[0].clone();
    assert_eq!(deps_field.field_type, FieldType::SingleSelect as i64);
    assert_eq!(dependencies.group_settings.len(), 1);
    let group_setting_map: GroupSettingMap = dependencies.group_settings[0].clone();
    let group_setting = GroupSetting::try_from(group_setting_map).unwrap();
    assert_eq!(group_setting.groups.len(), 1);
    assert_eq!(group_setting.groups[0].id, deps_field.id);

    let select_option = SelectOption::with_color("Done", SelectOptionColor::Purple);
    let options = vec![select_option];
    let card_status_option_ids: Vec<String> =
      options.iter().map(|option| option.id.clone()).collect();
    let mut card_status_options = SingleSelectTypeOption::default();
    card_status_options.options.extend(options);
    let mut card_status_field = Field::new(
      gen_field_id(),
      "Status".to_string(),
      FieldType::SingleSelect.into(),
      false,
    );
    card_status_field.type_options.insert(
      FieldType::SingleSelect.to_string(),
      card_status_options.into(),
    );
    let fields = vec![card_status_field.clone()];
    let dependencies =
      resolve_dependencies_when_create_database_linked_view(database_layout, &fields).unwrap();
    assert!(dependencies.deps_fields.is_empty());
    assert_eq!(dependencies.group_settings.len(), 1);
    let group_setting_map: GroupSettingMap = dependencies.group_settings[0].clone();
    let group_setting = GroupSetting::try_from(group_setting_map).unwrap();
    assert_eq!(group_setting.groups.len(), 2);
    assert_eq!(group_setting.groups[0].id, card_status_field.id);
    assert_eq!(group_setting.groups[1].id, card_status_option_ids[0]);
  }

  #[test]
  fn test_resolve_dependencies_when_create_database_linked_view_calendar() {
    let database_layout = DatabaseLayout::Calendar;
    let fields: Vec<Field> = vec![];
    let dependencies =
      resolve_dependencies_when_create_database_linked_view(database_layout, &fields).unwrap();
    assert_eq!(dependencies.deps_fields.len(), 1);
    assert_eq!(
      dependencies.deps_fields[0].field_type,
      FieldType::DateTime as i64
    );
    let date_field = Field::from_field_type("datetime", FieldType::DateTime, false);
    let fields: Vec<Field> = vec![
      Field::from_field_type("title", FieldType::RichText, true),
      date_field.clone(),
    ];
    let dependencies =
      resolve_dependencies_when_create_database_linked_view(database_layout, &fields).unwrap();
    assert!(dependencies.deps_fields.is_empty());
    let layout_setting: LayoutSetting = dependencies
      .layout_settings
      .get(&DatabaseLayout::Calendar)
      .unwrap()
      .to_owned();
    let calendar_layout_setting = CalendarLayoutSetting::from(layout_setting);
    assert_eq!(calendar_layout_setting.field_id, date_field.id);
  }
}
