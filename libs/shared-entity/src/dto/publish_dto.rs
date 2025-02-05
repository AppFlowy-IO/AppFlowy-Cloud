use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::workspace_dto::{ViewIcon, ViewLayout};

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
  pub database_row_collabs: HashMap<String, Vec<u8>>,

  /// The encoded collab data for the documents inside the database rows
  /// It's not used for now
  pub database_row_document_collabs: HashMap<String, Vec<u8>>,

  /// Visible view ids
  pub visible_database_view_ids: Vec<String>,

  /// Relation view id map
  pub database_relations: HashMap<String, String>,
}

#[derive(Default, Deserialize, Serialize, Clone, Debug, Eq, PartialEq)]
pub struct DuplicatePublishedPageResponse {
  pub view_id: String,
}
