use anyhow::Context;
use database_entity::database_error::DatabaseError;
use sqlx::postgres::PgArguments;
use sqlx::{Arguments, Executor, PgPool, Postgres};
use tracing::{instrument, warn};
use uuid::Uuid;

#[instrument(skip_all, err)]
pub async fn update_user(
  pool: &PgPool,
  user_uuid: &uuid::Uuid,
  name: Option<String>,
  email: Option<String>,
) -> Result<(), DatabaseError> {
  let mut set_clauses = Vec::new();
  let mut args = PgArguments::default();
  let mut args_num = 0;

  if let Some(n) = name {
    args_num += 1;
    set_clauses.push(format!("name = ${}", args_num));
    args.add(n);
  }

  if let Some(e) = email {
    args_num += 1;
    set_clauses.push(format!("email = ${}", args_num));
    args.add(e);
  }

  if set_clauses.is_empty() {
    warn!("No update params provided");
    return Ok(());
  }

  // where
  args_num += 1;
  let query = format!(
    "UPDATE af_user SET {} WHERE uuid = ${}",
    set_clauses.join(", "),
    args_num
  );
  args.add(user_uuid);

  sqlx::query_with(&query, args).execute(pool).await?;
  Ok(())
}

/// Attempts to create a new user in the database if they do not already exist.
///
/// This function will:
/// - Insert a new user record into the `af_user` table if the email is unique.
/// - If the user is newly created, it will also:
///   - Create a new workspace for the user in the `af_workspace` table.
///   - Assign the user a role in the `af_workspace_member` table.
///   - Add the user to the `af_collab_member` table with the appropriate permissions.
///
#[instrument(skip_all, err)]
pub async fn create_user_if_not_exists(
  pool: &PgPool,
  user_uuid: &uuid::Uuid,
  email: &str,
  name: &str,
) -> Result<bool, DatabaseError> {
  let affected_rows = sqlx::query!(
    r#"
        WITH ins_user AS (
            INSERT INTO af_user (uuid, email, name)
            VALUES ($1, $2, $3) 
            ON CONFLICT(email) DO NOTHING
            RETURNING uid
        ),
        owner_role AS (
            SELECT id FROM af_roles WHERE name = 'Owner'
        ),
        ins_workspace AS (
            INSERT INTO af_workspace (owner_uid)
            SELECT uid FROM ins_user
            RETURNING workspace_id, owner_uid
        ),
        ins_collab_member AS (
            INSERT INTO af_collab_member (uid, oid, permission_id)
            SELECT ins_workspace.owner_uid,
                   ins_workspace.workspace_id::TEXT, 
                   (SELECT permission_id FROM af_role_permissions WHERE role_id = owner_role.id)
            FROM ins_workspace, owner_role
        ),
        ins_workspace_member AS (
            INSERT INTO af_workspace_member (uid, role_id, workspace_id)
            SELECT ins_workspace.owner_uid, owner_role.id, ins_workspace.workspace_id
            FROM ins_workspace, owner_role
        )
        SELECT COUNT(*) FROM ins_user;
        "#,
    user_uuid,
    email,
    name
  )
  .fetch_one(pool)
  .await
  .context(format!(
    "Fail to insert user with uuid: {}, name: {}, email: {}",
    user_uuid, name, email
  ))?;
  Ok(affected_rows.count.unwrap_or(0) > 0)
}

pub async fn select_uid_from_uuid<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
) -> Result<i64, DatabaseError> {
  let uid = sqlx::query!(
    r#"
      SELECT uid FROM af_user WHERE uuid = $1
    "#,
    user_uuid
  )
  .fetch_one(executor)
  .await?
  .uid;
  Ok(uid)
}
pub async fn select_uid_from_email<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  email: &str,
) -> Result<i64, DatabaseError> {
  let uid = sqlx::query!(
    r#"
      SELECT uid FROM af_user WHERE email = $1
    "#,
    email
  )
  .fetch_one(executor)
  .await?
  .uid;
  Ok(uid)
}
