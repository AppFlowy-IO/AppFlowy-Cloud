use super::{Action, ObjectType};
use async_trait::async_trait;
use casbin::error::AdapterError;
use casbin::Adapter;
use casbin::Filter;
use casbin::Model;
use casbin::Result;
use database::collab::select_all_collab_members;
use database::workspace::select_all_workspace_members;
use database_entity::dto::AFAccessLevel;
use database_entity::dto::AFCollabMember;
use database_entity::pg_row::AFWorkspaceMemberRow;
use sqlx::PgPool;

/// Implmentation of [`casbin::Adapter`] for access control authorisation.
/// Access control policies that are managed by workspace and collab CRUD.
pub struct PgAdapter {
  pg_pool: PgPool,
}

impl PgAdapter {
  pub fn new(pg_pool: PgPool) -> Self {
    Self { pg_pool }
  }
}

fn create_collab_policies(collab_members: Vec<(String, Vec<AFCollabMember>)>) -> Vec<Vec<String>> {
  let mut policies: Vec<Vec<String>> = Vec::new();
  for (oid, members) in collab_members {
    for m in members {
      let p = [
        m.uid.to_string(),
        ObjectType::Collab(&oid).to_string(),
        i32::from(m.permission.access_level).to_string(),
      ]
      .to_vec();
      policies.push(p);
    }
  }

  policies
}

fn create_workspace_policies(
  workspace_members: Vec<(String, Vec<AFWorkspaceMemberRow>)>,
) -> Vec<Vec<String>> {
  let mut policies: Vec<Vec<String>> = Vec::new();
  for (oid, members) in workspace_members {
    for m in members {
      let p = [
        m.uid.to_string(),
        ObjectType::Workspace(&oid).to_string(),
        i32::from(m.role).to_string(),
      ]
      .to_vec();
      policies.push(p);
    }
  }

  policies
}

#[async_trait]
impl Adapter for PgAdapter {
  async fn load_policy(&mut self, model: &mut dyn Model) -> Result<()> {
    let workspace_members = select_all_workspace_members(&self.pg_pool)
      .await
      .map_err(|err| AdapterError(Box::new(err)))?;

    let workspace_policies = create_workspace_policies(workspace_members);
    // Policy definition `p` of type `p`. See `model.conf`
    model.add_policies("p", "p", workspace_policies);

    let collab_members = select_all_collab_members(&self.pg_pool)
      .await
      .map_err(|err| AdapterError(Box::new(err)))?;

    let collab_policies = create_collab_policies(collab_members);
    // Policy definition `p` of type `p`. See `model.conf`
    model.add_policies("p", "p", collab_policies);

    // Grouping definition of role to action.
    let af_access_levels = [
      AFAccessLevel::ReadOnly,
      AFAccessLevel::ReadAndComment,
      AFAccessLevel::ReadAndWrite,
      AFAccessLevel::FullAccess,
    ];
    let mut grouping_policies = Vec::new();
    for level in af_access_levels {
      // All levels can read
      grouping_policies.push([i32::from(level).to_string(), Action::Read.to_string()].to_vec());
      if level.can_write() {
        grouping_policies.push([i32::from(level).to_string(), Action::Write.to_string()].to_vec());
      }
      if level.can_delete() {
        grouping_policies.push([i32::from(level).to_string(), Action::Delete.to_string()].to_vec());
      }
    }

    // Grouping definition `g` of type `g`. See `model.conf`
    model.add_policies("g", "g", grouping_policies);

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

#[cfg(test)]
mod tests {
  use anyhow::{anyhow, Context};
  use casbin::{CoreApi, DefaultModel, Enforcer};
  use database_entity::dto::AFRole;
  use shared_entity::dto::workspace_dto::CreateWorkspaceMember;

  use crate::biz;
  use crate::biz::casbin::tests;

  use super::*;

  #[sqlx::test(migrations = false)]
  async fn test_create_user(pool: PgPool) -> anyhow::Result<()> {
    tests::setup_db(&pool).await?;

    let user = tests::create_user(&pool).await?;

    // Get workspace details
    let workspace = database::workspace::select_user_workspace(&pool, &user.uuid)
      .await?
      .into_iter()
      .next()
      .ok_or(anyhow!("workspace should be created"))?;

    let model = DefaultModel::from_str(tests::MODEL_CONF).await?;
    let enforcer = Enforcer::new(model, PgAdapter::new(pool.clone())).await?;

    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Workspace(&workspace.workspace_id.to_string()).to_string(),
        i32::from(AFRole::Owner).to_string(),
      ))
      .context("user should be owner of its workspace")?);

    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
        i32::from(AFAccessLevel::FullAccess).to_string(),
      ))
      .context("user should have full access of its collab")?);

    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
        Action::Read.to_string(),
      ))
      .context("user should be able to read its collab")?);

    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
        Action::Write.to_string(),
      ))
      .context("user should be able to write its collab")?);

    assert!(enforcer
      .enforce((
        user.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
        Action::Delete.to_string(),
      ))
      .context("user should be able to delete its collab")?);

    Ok(())
  }

  #[sqlx::test(migrations = false)]
  async fn test_add_users_to_workspace(pool: PgPool) -> anyhow::Result<()> {
    tests::setup_db(&pool).await?;

    let user_main = tests::create_user(&pool).await?;
    let user_owner = tests::create_user(&pool).await?;
    let user_member = tests::create_user(&pool).await?;
    let user_guest = tests::create_user(&pool).await?;

    // Get workspace details
    let workspace = database::workspace::select_user_workspace(&pool, &user_main.uuid)
      .await?
      .into_iter()
      .next()
      .ok_or(anyhow!("workspace should be created"))?;

    let members = vec![
      CreateWorkspaceMember {
        email: user_owner.email.clone(),
        role: AFRole::Owner,
      },
      CreateWorkspaceMember {
        email: user_member.email.clone(),
        role: AFRole::Member,
      },
      CreateWorkspaceMember {
        email: user_guest.email.clone(),
        role: AFRole::Guest,
      },
    ];
    let _ = biz::workspace::ops::add_workspace_members(
      &pool,
      &user_main.uuid,
      &workspace.workspace_id,
      members,
    )
    .await
    .context("adding users to workspace")?;

    let model = DefaultModel::from_str(tests::MODEL_CONF).await?;
    let enforcer = Enforcer::new(model, PgAdapter::new(pool.clone())).await?;

    {
      // Owner
      let user = user_owner;
      assert!(enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          i32::from(AFAccessLevel::FullAccess).to_string(),
        ))
        .context("owner should have full access of its collab")?);
      assert!(enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          Action::Read.to_string(),
        ))
        .context("user should be able to read its collab")?);

      assert!(enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          Action::Write.to_string(),
        ))
        .context("user should be able to write its collab")?);

      assert!(enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          Action::Delete.to_string(),
        ))
        .context("user should be able to delete its collab")?);
    }

    {
      // Member
      let user = user_member;
      assert!(enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          i32::from(AFAccessLevel::ReadAndWrite).to_string(),
        ))
        .context("member should have read write access of its collab")?);
      assert!(enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          Action::Read.to_string(),
        ))
        .context("user should be able to read its collab")?);

      assert!(enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          Action::Write.to_string(),
        ))
        .context("user should be able to write its collab")?);

      assert!(!enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          Action::Delete.to_string(),
        ))
        .context("user should not be able to delete its collab")?);
    }

    {
      // Guest
      let user = user_guest;
      assert!(enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          i32::from(AFAccessLevel::ReadOnly).to_string(),
        ))
        .context("guest should have read only access of its collab")?);
      assert!(enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          Action::Read.to_string(),
        ))
        .context("user should not be able to read its collab")?);

      assert!(!enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          Action::Write.to_string(),
        ))
        .context("user should not be able to write its collab")?);

      assert!(!enforcer
        .enforce((
          user.uid.to_string(),
          ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
          Action::Delete.to_string(),
        ))
        .context("user should not be able to delete its collab")?);
    }

    Ok(())
  }

  #[sqlx::test(migrations = false)]
  async fn test_reload_policy_after_adding_user_to_workspace(pool: PgPool) -> anyhow::Result<()> {
    tests::setup_db(&pool).await?;

    let user_owner = tests::create_user(&pool).await?;
    let user_member = tests::create_user(&pool).await?;

    // Get workspace details
    let workspace = database::workspace::select_user_workspace(&pool, &user_owner.uuid)
      .await?
      .into_iter()
      .next()
      .ok_or(anyhow!("workspace should be created"))?;

    // Create enforcer before adding user to workspace
    let model = DefaultModel::from_str(tests::MODEL_CONF).await?;
    let mut enforcer = Enforcer::new(model, PgAdapter::new(pool.clone())).await?;

    let members = vec![CreateWorkspaceMember {
      email: user_member.email.clone(),
      role: AFRole::Member,
    }];
    let _ = biz::workspace::ops::add_workspace_members(
      &pool,
      &user_owner.uuid,
      &workspace.workspace_id,
      members,
    )
    .await
    .context("adding users to workspace")?;

    assert!(!enforcer
      .enforce((
        user_member.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
        i32::from(AFAccessLevel::ReadAndWrite).to_string(),
      ))
      .context("member should not have read write access to collab before reload")?);

    enforcer.load_policy().await?;

    assert!(enforcer
      .enforce((
        user_member.uid.to_string(),
        ObjectType::Collab(&workspace.workspace_id.to_string()).to_string(),
        i32::from(AFAccessLevel::ReadAndWrite).to_string(),
      ))
      .context("member should have read write access to collab")?);

    Ok(())
  }
}
