use app_error::AppError;
use sqlx::postgres::PgArguments;
use sqlx::types::JsonValue;
use sqlx::{Arguments, Executor, PgPool, Postgres};
use tracing::{instrument, warn};
use uuid::Uuid;

/// Updates the user's details in the `af_user` table.
///
/// This function allows for updating the user's name, email, and metadata based on the provided UUID.
/// If the `metadata` is provided, it merges the new metadata with the existing one, with the new values
/// overriding the old ones in case of conflicts.
///
/// # Arguments
///
/// * `pool` - A reference to the database connection pool.
/// * `user_uuid` - The UUID of the user to be updated.
/// * `name` - An optional new name for the user.
/// * `email` - An optional new email for the user.
/// * `metadata` - An optional JSON value containing new metadata for the user.
///
#[instrument(skip_all, err)]
#[inline]
pub async fn update_user(
  pool: &PgPool,
  user_uuid: &uuid::Uuid,
  name: Option<String>,
  email: Option<String>,
  metadata: Option<JsonValue>,
) -> Result<(), AppError> {
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

  if let Some(m) = metadata {
    args_num += 1;
    // Merge existing metadata with new metadata
    set_clauses.push(format!("metadata = metadata || ${}", args_num));
    args.add(m);
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
#[instrument(skip(executor), err)]
#[inline]
pub async fn create_user<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  uid: i64,
  user_uuid: &Uuid,
  email: &str,
  name: &str,
) -> Result<(), AppError> {
  let row = sqlx::query!(
    r#"
    WITH ins_user AS (
        INSERT INTO af_user (uid, uuid, email, name)
        VALUES ($1, $2, $3, $4) 
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
    uid,
    user_uuid,
    email,
    name
  )
  .fetch_one(executor)
  .await?;

  debug_assert!(row.count.unwrap_or(0) <= 1, "More than one user inserted");
  Ok(())
}

#[inline]
#[instrument(level = "trace", skip(executor), err)]
pub async fn select_uid_from_uuid<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
) -> Result<i64, AppError> {
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

#[inline]
pub async fn select_uid_from_email<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  email: &str,
) -> Result<i64, AppError> {
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

#[inline]
pub async fn is_user_exist<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  user_uuid: &Uuid,
) -> Result<bool, AppError> {
  let exists = sqlx::query_scalar!(
    r#"
    SELECT EXISTS(
      SELECT 1 
      FROM af_user 
      WHERE uuid = $1
    ) AS user_exists;
  "#,
    user_uuid
  )
  .fetch_one(executor)
  .await?;

  Ok(exists.unwrap_or(false))
}
