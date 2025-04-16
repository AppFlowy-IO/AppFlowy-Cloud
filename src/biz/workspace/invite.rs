use app_error::AppError;
use database::workspace::{
  insert_workspace_invite_code, select_invitation_code_info, select_invited_workspace_id,
  upsert_workspace_member_uid,
};
use rand::{distributions::Alphanumeric, Rng};
use sqlx::PgPool;
use uuid::Uuid;

use database_entity::dto::{AFRole, InvitationCodeInfo, WorkspaceInviteToken};

const INVITE_LINK_CODE_LENGTH: usize = 16;

pub async fn generate_workspace_invite_token(
  pg_pool: &PgPool,
  workspace_id: &Uuid,
  validity_period_hours: Option<i64>,
) -> Result<WorkspaceInviteToken, AppError> {
  let code = generate_workspace_invite_code();
  let expires_at = validity_period_hours.map(|v| chrono::Utc::now() + chrono::Duration::hours(v));
  insert_workspace_invite_code(pg_pool, workspace_id, &code, expires_at.as_ref()).await?;

  Ok(WorkspaceInviteToken { code })
}

fn generate_workspace_invite_code() -> String {
  let rng = rand::thread_rng();
  rng
    .sample_iter(&Alphanumeric)
    .take(INVITE_LINK_CODE_LENGTH)
    .map(char::from)
    .collect()
}

pub async fn join_workspace_invite_by_code(
  pg_pool: &PgPool,
  invitation_code: &str,
  uid: i64,
) -> Result<Uuid, AppError> {
  let invited_workspace_id = select_invited_workspace_id(pg_pool, invitation_code).await?;
  upsert_workspace_member_uid(pg_pool, &invited_workspace_id, uid, AFRole::Member).await?;
  Ok(invited_workspace_id)
}

pub async fn get_invitation_code_info(
  pg_pool: &PgPool,
  invitation_code: &str,
  uid: i64,
) -> Result<InvitationCodeInfo, AppError> {
  let info_list = select_invitation_code_info(pg_pool, invitation_code, uid).await?;
  info_list
    .into_iter()
    .next()
    .ok_or(AppError::InvalidInvitationCode)
}
