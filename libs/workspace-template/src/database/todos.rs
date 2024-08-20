use std::sync::Arc;

use anyhow::Error;
use async_trait::async_trait;
use collab::preclude::{Collab, Map, MapRef, ToJson, TransactionMut};
use collab::util::MapExt;
use collab_database::database::{
  timestamp, Database, DatabaseContext, DatabaseData, FIELDS, METAS, VIEWS,
};
use collab_database::fields::{Field, FieldBuilder};
use collab_database::meta::DATABASE_INLINE_VIEW;
use collab_database::views::{
  CreateDatabaseParams, CreateViewParams, DatabaseView, FieldOrder, RowOrder, ViewBuilder,
};
use collab_entity::define::{DATABASE, DATABASE_ID};
use collab_entity::CollabType;
use collab_folder::{CollabOrigin, ViewLayout};
use tokio::sync::RwLock;

use crate::hierarchy_builder::WorkspaceViewBuilder;
use crate::{TemplateData, WorkspaceTemplate};

pub struct ToDosDatabaseTemplate;

#[async_trait]
impl WorkspaceTemplate for ToDosDatabaseTemplate {
  fn layout(&self) -> ViewLayout {
    ViewLayout::Board
  }

  async fn create(&self, object_id: String) -> anyhow::Result<TemplateData> {
    println!("create database: {}", object_id);
    let data = tokio::task::spawn_blocking(|| {
      // 1. read the create database params from the assets
      let create_database_params = todos_database_data().unwrap();
      let database_id = create_database_params.database_id.clone();

      // 2. create a new collab with the create database params
      let collab = create_database_collab(object_id.clone(), create_database_params)?;
      let data =
        collab.encode_collab_v1(|collab| CollabType::Database.validate_require_data(collab))?;

      // 3. save it to the cloud
      Ok::<_, anyhow::Error>(TemplateData {
        object_id,
        object_type: CollabType::Database,
        object_data: data,
        database_id: Some(database_id),
      })
    })
    .await??;
    Ok(data)
  }

  async fn create_workspace_view(
    &self,
    _uid: i64,
    workspace_view_builder: Arc<RwLock<WorkspaceViewBuilder>>,
  ) -> anyhow::Result<TemplateData> {
    let view_id = workspace_view_builder
      .write()
      .await
      .with_view_builder(|view_builder| async {
        view_builder
          .with_layout(ViewLayout::Board)
          .with_name("To-Dos")
          .with_icon("📝")
          .build()
      })
      .await;

    self.create(view_id).await
  }
}

pub fn todos_database_data() -> Result<CreateDatabaseParams, Error> {
  let json_str = include_str!("../../assets/to-dos.json");
  let database_data = serde_json::from_str::<DatabaseData>(json_str)?;
  println!("database_data: {:?}", serde_json::to_string(&database_data));
  let create_database_params = CreateDatabaseParams::from_database_data(database_data);
  println!("create_database_params: {:?}", create_database_params);
  Ok(create_database_params)
}

pub fn create_database_collab(
  object_id: String,
  params: CreateDatabaseParams,
) -> Result<Collab, Error> {
  let CreateDatabaseParams {
    database_id,
    rows,
    fields,
    inline_view_id,
    mut views,
  } = params;

  let mut collab = Collab::new_with_origin(CollabOrigin::Empty, &database_id, vec![], false);
  let mut txn = collab.context.transact_mut();
  let root = collab.data.get_or_init_map(&mut txn, DATABASE);

  root.insert(&mut txn, DATABASE_ID, database_id);

  // row_orders
  let row_orders = rows.iter().map(RowOrder::from).collect::<Vec<RowOrder>>();

  // field_orders
  let field_orders = fields
    .iter()
    .map(FieldOrder::from)
    .collect::<Vec<FieldOrder>>();

  // fields
  let mut fields_map = root.get_or_init_map(&mut txn, FIELDS);
  for field in fields {
    insert_field(&mut fields_map, &mut txn, field);
  }

  println!("fields_map: {:?}", fields_map.to_json(&txn));

  // views
  let mut views_map = root.get_or_init_map(&mut txn, VIEWS);
  let inline_view = views
    .iter()
    .find(|view| view.view_id == inline_view_id)
    .unwrap();
  // for view in views {
  //   println!("insert view: {:?}, object_id: {}", view, object_id);
  //   insert_view(
  //     &mut views_map,
  //     &mut txn,
  //     object_id.clone(),
  //     view,
  //     field_orders.clone(),
  //     row_orders.clone(),
  //   );
  // }

  insert_view(
    &mut views_map,
    &mut txn,
    object_id.clone(),
    inline_view.clone(),
    field_orders,
    row_orders,
  );

  println!("view_maps: {:?}", views_map.to_json(&txn));

  // inline view id
  let metas = root.get_or_init_map(&mut txn, METAS);
  metas.insert(&mut txn, DATABASE_INLINE_VIEW, inline_view_id.clone());

  println!("metas: {:?}", metas.to_json(&txn));

  println!("root: {:?}", root.to_json(&txn));

  drop(txn);

  Ok(collab)
}

/// Insert a field into the map with a transaction
pub fn insert_field(fields_map: &mut MapRef, txn: &mut TransactionMut, field: Field) {
  let map_ref = fields_map.get_or_init(txn, field.id.as_str());
  FieldBuilder::new(&field.id, txn, map_ref)
    .update(|update| {
      update
        .set_name(field.name)
        .set_created_at(timestamp())
        .set_last_modified(timestamp())
        .set_primary(field.is_primary)
        .set_field_type(field.field_type)
        .set_type_options(field.type_options);
    })
    .done();
}

/// Insert a view into the map with a transaction
pub fn insert_view(
  views_map: &mut MapRef,
  txn: &mut TransactionMut,
  object_id: String,
  params: CreateViewParams,
  field_orders: Vec<FieldOrder>,
  row_orders: Vec<RowOrder>,
) {
  let map_ref = views_map.get_or_init(txn, params.view_id.as_str());
  ViewBuilder::new(txn, map_ref)
    .update(|update| {
      update
        .set_view_id(&params.view_id)
        .set_database_id(params.database_id)
        .set_name(params.name)
        .set_created_at(params.created_at)
        .set_modified_at(params.modified_at)
        .set_layout_settings(params.layout_settings)
        .set_layout_type(params.layout)
        .set_field_settings(params.field_settings)
        .set_filters(params.filters)
        .set_groups(params.group_settings)
        .set_sorts(params.sorts)
        .set_field_orders(field_orders)
        .set_row_orders(row_orders);
    })
    .done();
}
