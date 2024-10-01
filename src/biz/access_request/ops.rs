use std::{ops::DerefMut, sync::Arc};

use anyhow::Context;
use app_error::AppError;
use appflowy_collaborate::collab::storage::CollabAccessControlStorage;
use database::{
  access_request::{
    insert_new_access_request, select_access_request_by_request_id, update_access_request_status,
  },
  collab::GetCollabOrigin,
  pg_row::AFAccessRequestStatusColumn,
  workspace::upsert_workspace_member_with_txn,
};
use database_entity::dto::AFRole;
use shared_entity::dto::access_request_dto::{AccessRequest, AccessRequestView};
use sqlx::PgPool;
use uuid::Uuid;

use crate::{
  biz::collab::{
    folder_view::{to_dto_view_icon, to_view_layout},
    ops::get_latest_collab_folder,
  },
  mailer::{Mailer, WorkspaceAccessRequestMailerParam},
};

pub async fn create_access_request(
  pg_pool: &PgPool,
  mailer: Mailer,
  appflowy_web_url: &str,
  workspace_id: Uuid,
  view_id: Uuid,
  uid: i64,
) -> Result<Uuid, AppError> {
  let request_id = insert_new_access_request(pg_pool, workspace_id, view_id, uid).await?;
  let access_request = select_access_request_by_request_id(pg_pool, request_id).await?;
  let cloned_mailer = mailer.clone();
  let approve_url = format!(
    "{}/app/approve-request?request_id={}",
    appflowy_web_url, request_id
  );
  let email = access_request.workspace.owner_email.clone();
  let recipient_name = access_request.workspace.owner_name.clone();
  // use default icon until we have workspace icon
  let workspace_icon_url =
    "https://miro.medium.com/v2/resize:fit:2400/1*mTPfm7CwU31-tLhtLNkyJw.png".to_string();
  let user_icon_url =
    "https://cdn.pixabay.com/photo/2015/10/05/22/37/blank-profile-picture-973460_1280.png"
      .to_string();
  tokio::spawn(async move {
    if let Err(err) = cloned_mailer
      .send_workspace_access_request(
        &recipient_name,
        &email,
        WorkspaceAccessRequestMailerParam {
          user_icon_url,
          username: access_request.requester.name,
          workspace_name: access_request.workspace.workspace_name,
          workspace_icon_url,
          workspace_member_count: access_request.workspace.member_count.unwrap_or(0),
          approve_url: approve_url.to_string(),
        },
      )
      .await
    {
      tracing::error!("Failed to send access request email: {:?}", err);
    };
  });
  Ok(request_id)
}

pub async fn get_access_request(
  pg_pool: &PgPool,
  collab_storage: Arc<CollabAccessControlStorage>,
  access_request_id: Uuid,
) -> Result<AccessRequest, AppError> {
  let access_request_with_view_id =
    select_access_request_by_request_id(pg_pool, access_request_id).await?;
  let folder = get_latest_collab_folder(
    collab_storage,
    GetCollabOrigin::Server,
    &access_request_with_view_id
      .workspace
      .workspace_id
      .to_string(),
  )
  .await?;
  let view = folder.get_view(&access_request_with_view_id.view_id.to_string());
  let access_request_view = view
    .map(|v| AccessRequestView {
      view_id: v.id.clone(),
      name: v.name.clone(),
      icon: v.icon.as_ref().map(|icon| to_dto_view_icon(icon.clone())),
      layout: to_view_layout(&v.layout),
    })
    .ok_or(AppError::MissingView(format!(
      "the view {} is missing",
      access_request_with_view_id.view_id
    )))?;
  let access_request = AccessRequest {
    request_id: access_request_with_view_id.request_id,
    workspace: access_request_with_view_id.workspace,
    requester: access_request_with_view_id.requester,
    view: access_request_view,
    status: access_request_with_view_id.status,
    created_at: access_request_with_view_id.created_at,
  };
  Ok(access_request)
}

pub async fn approve_or_reject_access_request(
  pg_pool: &PgPool,
  request_id: Uuid,
  uid: i64,
  user_uuid: Uuid,
  is_approved: bool,
) -> Result<(), AppError> {
  let access_request = select_access_request_by_request_id(pg_pool, request_id).await?;
  if access_request.workspace.owner_uid != uid {
    return Err(AppError::NotEnoughPermissions {
      user: user_uuid.to_string(),
      action: "approve access request".to_string(),
    });
  }

  let mut txn = pg_pool.begin().await.context("approving request")?;
  let role = AFRole::Member;
  if is_approved {
    upsert_workspace_member_with_txn(
      &mut txn,
      &access_request.workspace.workspace_id,
      &access_request.requester.email,
      role,
    )
    .await?;
  }
  let status = if is_approved {
    AFAccessRequestStatusColumn::Approved
  } else {
    AFAccessRequestStatusColumn::Rejected
  };
  update_access_request_status(txn.deref_mut(), request_id, status).await?;
  txn.commit().await.context("committing transaction")?;
  Ok(())
}