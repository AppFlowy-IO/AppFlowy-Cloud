use std::sync::Arc;
use std::vec;

use anyhow::Error;
use async_trait::async_trait;
use collab::preclude::{Collab, Map, MapRef, ToJson, TransactionMut};
use collab::util::MapExt;
use collab_database::database::{timestamp, DatabaseData, FIELDS, METAS, VIEWS};
use collab_database::fields::{Field, FieldBuilder};
use collab_database::meta::DATABASE_INLINE_VIEW;
use collab_database::rows::{RowBuilder, META};
use collab_database::views::{
  CreateDatabaseParams, CreateViewParams, FieldOrder, RowOrder, ViewBuilder,
};
use collab_entity::define::{DATABASE, DATABASE_ID, DATABASE_ROW_DATA};
use collab_entity::CollabType;
use collab_folder::{CollabOrigin, ViewLayout};
use tokio::sync::RwLock;

use crate::hierarchy_builder::WorkspaceViewBuilder;
use crate::{gen_view_id, TemplateData, WorkspaceTemplate};

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

  // the database collab object id is the database id
  let mut collab = Collab::new_with_origin(CollabOrigin::Empty, &database_id, vec![], false);
  let mut txn = collab.context.transact_mut();
  let root = collab.data.get_or_init_map(&mut txn, DATABASE);

  root.insert(&mut txn, DATABASE_ID, database_id.clone());

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

  // views
  let mut views_map = root.get_or_init_map(&mut txn, VIEWS);
  let inline_view = views
    .iter()
    .find(|view| view.view_id == inline_view_id)
    .unwrap();
  let linked_views = views
    .iter()
    .filter(|view| view.view_id != inline_view_id)
    .collect::<Vec<&CreateViewParams>>();

  // create inline view
  insert_view(
    &mut views_map,
    &mut txn,
    object_id.clone(),
    inline_view.clone(),
    field_orders.clone(),
    row_orders.clone(),
  );

  // inline view id
  let metas = root.get_or_init_map(&mut txn, METAS);
  metas.insert(&mut txn, DATABASE_INLINE_VIEW, object_id.clone());

  // create linked view
  for linked_view in linked_views {
    insert_view(
      &mut views_map,
      &mut txn,
      linked_view.view_id.clone(),
      linked_view.clone(),
      field_orders.clone(),
      row_orders.clone(),
    );
  }

  println!(
    "database_id: {}, collab: {:?}",
    database_id,
    collab.data.to_json(&txn)
  );

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
  let map_ref = views_map.get_or_init(txn, object_id.clone());
  ViewBuilder::new(txn, map_ref)
    .update(|update| {
      update
        .set_view_id(&object_id)
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

pub fn create_database_row_collabs(
  object_id: String,
  params: CreateDatabaseParams,
) -> Result<Vec<(String, Collab)>, Error> {
  let CreateDatabaseParams {
    database_id,
    rows,
    fields,
    inline_view_id,
    mut views,
  } = params;

  let mut collabs = vec![];

  for row in rows {
    let row_id = row.id.clone().to_string();
    // the database collab object id is the database id
    let mut collab = Collab::new_with_origin(CollabOrigin::Empty, &row_id, vec![], false);
    let mut txn = collab.context.transact_mut();

    // data
    let data = collab.data.get_or_init_map(&mut txn, DATABASE_ROW_DATA);
    let meta = collab.data.get_or_init_map(&mut txn, META);

    RowBuilder::new(&mut txn, data, meta)
      .update(|update| {
        update
          .set_row_id(row.id, row.database_id)
          .set_height(row.height)
          .set_visibility(row.visibility)
          .set_created_at(timestamp())
          .set_last_modified(timestamp())
          .set_cells(row.cells);
      })
      .done();

    println!(
      "row_id: {}, collab: {:?}",
      row_id,
      collab.data.to_json(&txn)
    );

    drop(txn);

    collabs.push((row_id, collab));
  }

  Ok(collabs)
}
