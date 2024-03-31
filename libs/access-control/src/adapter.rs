use crate::access::ObjectType;

use async_trait::async_trait;

use crate::metrics::AccessControlMetrics;
use casbin::Adapter;
use casbin::Filter;
use casbin::Model;
use casbin::Result;

use database::collab::select_collab_member_access_level;
use database::pg_row::AFCollabMemberAccessLevelRow;
use database::pg_row::AFWorkspaceMemberPermRow;
use database::workspace::select_workspace_member_perm_stream;

use crate::act::Acts;
use futures_util::stream::BoxStream;
use sqlx::PgPool;
use std::sync::Arc;
use std::time::Instant;
use tokio_stream::StreamExt;

/// Implementation of [`casbin::Adapter`] for access control authorisation.
/// Access control policies that are managed by workspace and collab CRUD.
pub struct PgAdapter {
  pg_pool: PgPool,
  access_control_metrics: Arc<AccessControlMetrics>,
}

impl PgAdapter {
  pub fn new(pg_pool: PgPool, access_control_metrics: Arc<AccessControlMetrics>) -> Self {
    Self {
      pg_pool,
      access_control_metrics,
    }
  }
}

async fn load_collab_policies(
  mut stream: BoxStream<'_, sqlx::Result<AFCollabMemberAccessLevelRow>>,
) -> Result<Vec<Vec<String>>> {
  let mut policies: Vec<Vec<String>> = Vec::new();

  while let Some(Ok(member_access_lv)) = stream.next().await {
    let uid = member_access_lv.uid;
    let object_type = ObjectType::Collab(&member_access_lv.oid);
    for act in member_access_lv.access_level.policy_acts() {
      let policy = [
        uid.to_string(),
        object_type.policy_object(),
        act.to_string(),
      ]
      .to_vec();
      policies.push(policy);
    }
  }

  Ok(policies)
}

/// Loads workspace policies from a given stream of workspace member permissions.
///
/// This function iterates over the stream of member permissions, constructing and accumulating
/// policies for each member. A policy is represented as a vector of strings containing the user ID,
/// object type (workspace), and action (derived from their role within the workspace). Additional
/// policies are added for roles with implicit permissions (e.g., owners implicitly have member and
/// guest permissions).
///
/// # Arguments
///
/// * `stream` - A stream of `sqlx::Result<AFWorkspaceMemberPermRow>` representing the database
///   query results for workspace member permissions.
///
/// # Returns
///
/// Returns a `Result<Vec<Vec<String>>>`, which is a vector of policies. Each policy is itself a
/// vector containing the user ID, policy object, and action as strings. In case of an error while
/// processing the stream, returns the error encapsulated within `Result`.
///
/// # Example Policy Vector
///
/// For a workspace owner with user ID `1` and workspace ID `123`, the function generates policies
/// such as:
///
/// ```ignore
/// [
///   ["1", "workspace:123", "owner"],
///   ["1", "workspace:123", "member"], // Implicit permission for owner
///   ["1", "workspace:123", "guest"],  // Implicit permission for owner
/// ]
/// ```
///
/// # Note
///
/// - The function handles additional policies for `Owner` and `Member` roles to include implicit
///   permissions. For example, an `Owner` implicitly has `Member` and `Guest` permissions, and a
///   `Member` implicitly has `Guest` permissions.
/// - The policy object is derived from the `ObjectType::Workspace`, and actions are derived from
///   member roles (`Owner`, `Member`, `Guest`) using the `to_action` method.
async fn load_workspace_policies(
  mut stream: BoxStream<'_, sqlx::Result<AFWorkspaceMemberPermRow>>,
) -> Result<Vec<Vec<String>>> {
  let mut policies: Vec<Vec<String>> = Vec::new();

  while let Some(Ok(member_permission)) = stream.next().await {
    let uid = member_permission.uid;
    let workspace_id = member_permission.workspace_id.to_string();
    let object_type = ObjectType::Workspace(&workspace_id);
    for act in member_permission.role.policy_acts() {
      let policy = vec![
        uid.to_string(),
        object_type.policy_object(),
        act.to_string(),
      ];
      policies.push(policy);
    }
  }

  Ok(policies)
}

#[async_trait]
impl Adapter for PgAdapter {
  async fn load_policy(&mut self, model: &mut dyn Model) -> Result<()> {
    let start = Instant::now();
    let workspace_member_perm_stream = select_workspace_member_perm_stream(&self.pg_pool);
    let workspace_policies = load_workspace_policies(workspace_member_perm_stream).await?;

    // Policy definition `p` of type `p`. See `model.conf`
    model.add_policies("p", "p", workspace_policies);

    let collab_member_access_lv_stream = select_collab_member_access_level(&self.pg_pool);
    let collab_policies = load_collab_policies(collab_member_access_lv_stream).await?;

    // Policy definition `p` of type `p`. See `model.conf`
    model.add_policies("p", "p", collab_policies);

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
