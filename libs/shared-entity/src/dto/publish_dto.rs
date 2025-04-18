use std::collections::HashMap;

use super::workspace_dto::{ViewIcon, ViewLayout};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Copied from AppFlowy-IO/AppFlowy/frontend/rust-lib/flowy-folder-pub/src/entities.rs
/// TODO(zack): make AppFlowy use from this crate instead
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishViewMeta {
  pub metadata: PublishViewMetaData,
  pub view_id: String,
  pub publish_name: String,
}

#[derive(Default, Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
pub struct PublishViewMetaData {
  pub view: PublishViewInfo,
  pub child_views: Vec<PublishViewInfo>,
  pub ancestor_views: Vec<PublishViewInfo>,
}

#[derive(Default, Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
pub struct PublishViewInfo {
  pub view_id: String,
  pub name: String,
  pub icon: Option<ViewIcon>,
  pub layout: ViewLayout,
  pub extra: Option<String>,
  pub created_by: Option<i64>,
  pub last_edited_by: Option<i64>,
  pub last_edited_time: i64,
  pub created_at: i64,
  pub child_views: Option<Vec<PublishViewInfo>>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PublishDatabasePayload {
  pub meta: PublishViewMeta,
  pub data: PublishDatabaseData,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq)]
pub struct PublishDatabaseData {
  /// The encoded collab data for the database itself
  pub database_collab: Vec<u8>,

  /// The encoded collab data for the database rows
  /// Use the row_id as the key
  pub database_row_collabs: HashMap<Uuid, Vec<u8>>,

  /// The encoded collab data for the documents inside the database rows
  /// It's not used for now
  pub database_row_document_collabs: HashMap<Uuid, Vec<u8>>,

  /// Visible view ids
  pub visible_database_view_ids: Vec<Uuid>,

  /// Relation view id map
  pub database_relations: HashMap<Uuid, Uuid>,
}

/// For fixing invalid published template with non-uuid relations
#[derive(Debug, Clone, Serialize, Deserialize, Default, Eq, PartialEq)]
pub struct PublishDatabaseDataWithNonUuidRelations {
  /// The encoded collab data for the database itself
  pub database_collab: Vec<u8>,

  /// The encoded collab data for the database rows
  /// Use the row_id as the key
  pub database_row_collabs: HashMap<Uuid, Vec<u8>>,

  /// The encoded collab data for the documents inside the database rows
  /// It's not used for now
  pub database_row_document_collabs: HashMap<Uuid, Vec<u8>>,

  /// Visible view ids
  pub visible_database_view_ids: Vec<Uuid>,

  /// Relation view id map. Usually database relation ids should only consist
  /// of uuids. However, it might be possible for a published database to contain
  /// relations which are non uuids, which we will need to filter out later.
  pub database_relations: HashMap<String, String>,
}

impl From<PublishDatabaseDataWithNonUuidRelations> for PublishDatabaseData {
  fn from(data: PublishDatabaseDataWithNonUuidRelations) -> Self {
    PublishDatabaseData {
      database_collab: data.database_collab,
      database_row_collabs: data.database_row_collabs,
      database_row_document_collabs: data.database_row_document_collabs,
      visible_database_view_ids: data.visible_database_view_ids,
      // Filter out all non-uuid relations
      database_relations: data
        .database_relations
        .into_iter()
        .filter_map(|(key, value)| {
          let key_uuid = Uuid::parse_str(&key).ok()?;
          let value_uuid = Uuid::parse_str(&value).ok()?;
          Some((key_uuid, value_uuid))
        })
        .collect(),
    }
  }
}

#[derive(Default, Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
pub struct DuplicatePublishedPageResponse {
  pub view_id: Uuid,
}
