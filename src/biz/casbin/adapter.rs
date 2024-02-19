use crate::biz::casbin::access_control::{Action, ObjectType, ToCasbinAction};
use crate::biz::casbin::enforcer::ActionCacheKey;
use async_trait::async_trait;

use crate::biz::casbin::metrics::AccessControlMetrics;
use casbin::Adapter;
use casbin::Filter;
use casbin::Model;
use casbin::Result;
use dashmap::DashMap;
use database::collab::select_collab_member_access_level;
use database::pg_row::AFCollabMemerAccessLevelRow;
use database::pg_row::AFWorkspaceMemberPermRow;
use database::workspace::select_workspace_member_perm_stream;
use database_entity::dto::{AFAccessLevel, AFRole};
use futures_util::stream::BoxStream;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Instant;
use tokio_stream::StreamExt;

/// Implementation of [`casbin::Adapter`] for access control authorisation.
/// Access control policies that are managed by workspace and collab CRUD.
pub struct PgAdapter {
  pg_pool: PgPool,
  action_cache: Arc<DashMap<ActionCacheKey, String>>,
  access_control_metrics: Arc<AccessControlMetrics>,
}

impl PgAdapter {
  pub fn new(
    pg_pool: PgPool,
    action_cache: Arc<DashMap<ActionCacheKey, String>>,
    access_control_metrics: Arc<AccessControlMetrics>,
  ) -> Self {
    Self {
      pg_pool,
      action_cache,
      access_control_metrics,
    }
  }
}

async fn load_collab_policies(
  action_cache: &Arc<DashMap<ActionCacheKey, String>>,
  mut stream: BoxStream<'_, sqlx::Result<AFCollabMemerAccessLevelRow>>,
) -> Result<Vec<Vec<String>>> {
  let mut policies: Vec<Vec<String>> = Vec::new();

  while let Some(Ok(member_access_lv)) = stream.next().await {
    let uid = member_access_lv.uid;
    let object_type = ObjectType::Collab(&member_access_lv.oid);
    let action = member_access_lv.access_level.to_action();
    action_cache.insert(ActionCacheKey::new(&uid, &object_type), action.clone());

    let policy = [uid.to_string(), object_type.to_object_id(), action].to_vec();
    policies.push(policy);
  }

  Ok(policies)
}

async fn load_workspace_policies(
  action_cache: &Arc<DashMap<ActionCacheKey, String>>,
  mut stream: BoxStream<'_, sqlx::Result<AFWorkspaceMemberPermRow>>,
) -> Result<Vec<Vec<String>>> {
  let mut policies: Vec<Vec<String>> = Vec::new();

  while let Some(Ok(member_permission)) = stream.next().await {
    let uid = member_permission.uid;
    let workspace_id = member_permission.workspace_id.to_string();
    let object_type = ObjectType::Workspace(&workspace_id);
    let action = member_permission.role.to_action();
    action_cache.insert(ActionCacheKey::new(&uid, &object_type), action.clone());

    let policy = [uid.to_string(), object_type.to_object_id(), action].to_vec();
    policies.push(policy);
  }

  Ok(policies)
}

#[async_trait]
impl Adapter for PgAdapter {
  async fn load_policy(&mut self, model: &mut dyn Model) -> Result<()> {
    let start = Instant::now();
    let workspace_member_perm_stream = select_workspace_member_perm_stream(&self.pg_pool);
    let workspace_policies =
      load_workspace_policies(&self.action_cache, workspace_member_perm_stream).await?;

    // Policy definition `p` of type `p`. See `model.conf`
    model.add_policies("p", "p", workspace_policies);

    let collab_member_access_lv_stream = select_collab_member_access_level(&self.pg_pool);
    let collab_policies =
      load_collab_policies(&self.action_cache, collab_member_access_lv_stream).await?;

    // Policy definition `p` of type `p`. See `model.conf`
    model.add_policies("p", "p", collab_policies);

    // Grouping definition of access level to action.
    let af_access_levels = [
      AFAccessLevel::ReadOnly,
      AFAccessLevel::ReadAndComment,
      AFAccessLevel::ReadAndWrite,
      AFAccessLevel::FullAccess,
    ];
    let mut grouping_policies = Vec::new();
    for level in af_access_levels {
      // All levels can read
      grouping_policies.push([level.to_action(), Action::Read.to_action()].to_vec());
      if level.can_write() {
        grouping_policies.push([level.to_action(), Action::Write.to_action()].to_vec());
      }
      if level.can_delete() {
        grouping_policies.push([level.to_action(), Action::Delete.to_action()].to_vec());
      }
    }

    let af_roles = [AFRole::Owner, AFRole::Member, AFRole::Guest];
    for role in af_roles {
      match role {
        AFRole::Owner => {
          grouping_policies.push([role.to_action(), Action::Write.to_action()].to_vec());
        },
        AFRole::Member => {
          grouping_policies.push([role.to_action(), Action::Read.to_action()].to_vec());
          grouping_policies.push([role.to_action(), Action::Write.to_action()].to_vec());
        },
        AFRole::Guest => {
          grouping_policies.push([role.to_action(), Action::Read.to_action()].to_vec());
        },
      }
    }

    // Grouping definition `g` of type `g`. See `model.conf`
    model.add_policies("g", "g", grouping_policies);
    self
      .access_control_metrics
      .record_load_all_policies_in_secs(start.elapsed().as_millis() as u64);

    Ok(())
  }

  async fn load_filtered_policy<'a>(&mut self, m: &mut dyn Model, _f: Filter<'a>) -> Result<()> {
    // No support for filtered.
    self.load_policy(m).await
  }
  async fn save_policy(&mut self, _m: &mut dyn Model) -> Result<()> {
    // unimplemented!()
    //
    // Adapter is used only for loading policies from database
    // since policies are managed by workspace and collab CRUD.
    Ok(())
  }
  async fn clear_policy(&mut self) -> Result<()> {
    // unimplemented!()
    //
    // Adapter is used only for loading policies from database
    // since policies are managed by workspace and collab CRUD.
    Ok(())
  }
  fn is_filtered(&self) -> bool {
    // No support for filtered.
    false
  }
  async fn add_policy(&mut self, _sec: &str, _ptype: &str, _rule: Vec<String>) -> Result<bool> {
    // unimplemented!()
    //
    // Adapter is used only for loading policies from database
    // since policies are managed by workspace and collab CRUD.
    Ok(true)
  }
  async fn add_policies(
    &mut self,
    _sec: &str,
    _ptype: &str,
    _rules: Vec<Vec<String>>,
  ) -> Result<bool> {
    // unimplemented!()
    //
    // Adapter is used only for loading policies from database
    // since policies are managed by workspace and collab CRUD.
    Ok(true)
  }
  async fn remove_policy(&mut self, _sec: &str, _ptype: &str, _rule: Vec<String>) -> Result<bool> {
    // unimplemented!()
    //
    // Adapter is used only for loading policies from database
    // since policies are managed by workspace and collab CRUD.
    Ok(true)
  }
  async fn remove_policies(
    &mut self,
    _sec: &str,
    _ptype: &str,
    _rules: Vec<Vec<String>>,
  ) -> Result<bool> {
    // unimplemented!()
    //
    // Adapter is used only for loading policies from database
    // since policies are managed by workspace and collab CRUD.
    Ok(true)
  }
  async fn remove_filtered_policy(
    &mut self,
    _sec: &str,
    _ptype: &str,
    _field_index: usize,
    _field_values: Vec<String>,
  ) -> Result<bool> {
    // unimplemented!()
    //
    // Adapter is used only for loading policies from database
    // since policies are managed by workspace and collab CRUD.
    Ok(true)
  }
}
