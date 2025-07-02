use database_entity::dto::{AFUserWithAvatar, AFWebUser};
use futures_util::stream::BoxStream;
use sqlx::postgres::PgArguments;
use sqlx::types::JsonValue;
use sqlx::{Arguments, Executor, PgPool, Postgres};
use tracing::{instrument, warn};
use uuid::Uuid;

use app_error::AppError;

use crate::pg_row::AFUserIdRow;

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
    args.add(n).map_err(|err| AppError::SqlxArgEncodingError {
      desc: format!("unable to encode user name for user {}", user_uuid),
      err,
    })?;
  }

  if let Some(e) = email {
    args_num += 1;
    set_clauses.push(format!("email = ${}", args_num));
    args.add(e).map_err(|err| AppError::SqlxArgEncodingError {
      desc: format!("unable to encode email for user {}", user_uuid),
      err,
    })?;
  }

  if let Some(m) = metadata {
    args_num += 1;
    // Merge existing metadata with new metadata
    set_clauses.push(format!("metadata = metadata || ${}", args_num));
    args.add(m).map_err(|err| AppError::SqlxArgEncodingError {
      desc: format!("unable to encode metadata for user {}", user_uuid),
      err,
    })?;
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
  args
    .add(user_uuid)
    .map_err(|err| AppError::SqlxArgEncodingError {
      desc: format!("unable to encode user uuid {}", user_uuid),
      err,
    })?;

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
/// # Returns
/// A `Result` containing the workspace_id of the user's newly created workspace
#[instrument(skip(executor), err)]
#[inline]
pub async fn create_user<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  uid: i64,
  user_uuid: &Uuid,
  email: &str,
  name: &str,
) -> Result<Uuid, AppError> {
  let name = {
    if name.is_empty() {
      email
    } else {
      name
    }
  };

  let row = sqlx::query!(
    r#"
    WITH ins_user AS (
        INSERT INTO af_user (uid, uuid, email, name)
        VALUES ($1, $2, $3, $4)
        RETURNING uid
    ),
    owner_role AS (
        SELECT id FROM af_roles WHERE name = 'Owner'
    ),
    ins_workspace AS (
        INSERT INTO af_workspace (owner_uid)
        SELECT uid FROM ins_user
        RETURNING workspace_id, owner_uid
    )
    SELECT workspace_id FROM ins_workspace;
    "#,
    uid,
    user_uuid,
    email,
    name
  )
  .fetch_one(executor)
  .await?;

  Ok(row.workspace_id)
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

pub fn select_all_uid_uuid<'a, E: Executor<'a, Database = Postgres> + 'a>(
  executor: E,
) -> BoxStream<'a, sqlx::Result<AFUserIdRow>> {
  sqlx::query_as!(AFUserIdRow, r#" SELECT uid, uuid FROM af_user"#,).fetch(executor)
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

#[inline]
pub async fn select_email_from_user_uuid(
  pool: &PgPool,
  user_uuid: &Uuid,
) -> Result<String, AppError> {
  let email = sqlx::query_scalar!(
    r#"
      SELECT email FROM af_user WHERE uuid = $1
    "#,
    user_uuid
  )
  .fetch_one(pool)
  .await?;
  Ok(email)
}

#[inline]
pub async fn select_email_from_user_uid(pool: &PgPool, user_uid: i64) -> Result<String, AppError> {
  let email = sqlx::query_scalar!(
    r#"
      SELECT email FROM af_user WHERE uid = $1
    "#,
    user_uid
  )
  .fetch_one(pool)
  .await?;
  Ok(email)
}

#[inline]
pub async fn select_name_from_uuid(pool: &PgPool, user_uuid: &Uuid) -> Result<String, AppError> {
  let email = sqlx::query_scalar!(
    r#"
      SELECT name FROM af_user WHERE uuid = $1
    "#,
    user_uuid
  )
  .fetch_one(pool)
  .await?;
  Ok(email)
}

pub async fn select_name_and_email_from_uuid(
  pool: &PgPool,
  user_uuid: &Uuid,
) -> Result<(String, String), AppError> {
  let row = sqlx::query!(
    r#"
    SELECT name, email FROM af_user WHERE uuid = $1
    "#,
    user_uuid
  )
  .fetch_one(pool)
  .await?;

  Ok((row.name, row.email))
}

pub async fn select_web_user_from_uid(
  pool: &PgPool,
  uid: i64,
) -> Result<Option<AFWebUser>, AppError> {
  let row = sqlx::query_as!(
    AFWebUser,
    r#"
    SELECT
      uuid,
      name,
      metadata ->> 'icon_url' AS avatar_url
    FROM af_user
    WHERE uid = $1
    "#,
    uid
  )
  .fetch_optional(pool)
  .await
  .map_err(|err| anyhow::anyhow!("Unable to get user detail for {}: {}", uid, err))?;

  Ok(row)
}

pub async fn select_user_with_avatar<'a, E: Executor<'a, Database = Postgres>>(
  executor: E,
  uid: i64,
) -> Result<AFUserWithAvatar, AppError> {
  let user = sqlx::query_as!(
    AFUserWithAvatar,
    r#"
      SELECT
        uuid,
        email,
        name,
        metadata ->> 'icon_url' AS avatar_url
      FROM af_user
      WHERE uid = $1;
    "#,
    uid
  )
  .fetch_one(executor)
  .await?;
  Ok(user)
}
