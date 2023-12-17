use crate::api::ws::CollabServerImpl;
use crate::biz;
use crate::biz::user::RealtimeUserImpl;
use crate::biz::workspace;
use crate::biz::workspace::access_control::WorkspaceAccessControl;
use crate::component::auth::jwt::UserUuid;
use crate::state::AppState;
use actix_web::web::Bytes;
use actix_web::web::{Data, Json, PayloadConfig};
use actix_web::Result;
use actix_web::{web, Scope};
use app_error::AppError;
use collab::core::collab_plugin::EncodedCollabV1;
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database::user::{select_uid_from_email, select_uid_from_uuid};
use database_entity::dto::*;
use prost::Message as ProstMessage;
use realtime::collaborate::CollabAccessControl;
use realtime::entities::{ClientMessage, RealtimeMessage};
use realtime_entity::realtime_proto::HttpRealtimeMessage;
use shared_entity::dto::workspace_dto::*;
use shared_entity::response::AppResponseError;
use shared_entity::response::{AppResponse, JsonAppResponse};
use sqlx::types::uuid;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{event, instrument};
use uuid::Uuid;

pub const WORKSPACE_ID_PATH: &str = "workspace_id";
pub const COLLAB_OBJECT_ID_PATH: &str = "object_id";

pub fn workspace_scope() -> Scope {
  web::scope("/api/workspace")
    .service(web::resource("list").route(web::get().to(list_handler)))
    .service(web::resource("{workspace_id}/open").route(web::put().to(open_workspace_handler)))
    .service(
      web::resource("{workspace_id}/member")
        .route(web::get().to(get_workspace_members_handler))
        .route(web::post().to(add_workspace_members_handler))
        .route(web::put().to(update_workspace_member_handler))
        .route(web::delete().to(remove_workspace_member_handler)),
    )
    .service(
      web::resource("{workspace_id}/collab/{object_id}")
        .app_data(
          PayloadConfig::new(5 * 1024 * 1024), // 5 MB
        )
        .route(web::post().to(create_collab_handler))
        .route(web::get().to(get_collab_handler))
        .route(web::put().to(update_collab_handler))
        .route(web::delete().to(delete_collab_handler)),
    )
    .service(
      web::resource("{workspace_id}/{object_id}/snapshot")
        .route(web::get().to(get_collab_snapshot_handler))
        .route(web::post().to(create_collab_snapshot_handler)),
    )
    .service(
      web::resource("{workspace_id}/{object_id}/snapshot/list")
        .route(web::get().to(get_all_collab_snapshot_list_handler)),
    )
    .service(
      web::resource("{workspace_id}/collab/{object_id}/member")
        .route(web::post().to(add_collab_member_handler))
        .route(web::get().to(get_collab_member_handler))
        .route(web::put().to(update_collab_member_handler))
        .route(web::delete().to(remove_collab_member_handler)),
    )
    .service(
      web::resource("{workspace_id}/collab/{object_id}/member/list")
        .route(web::get().to(get_collab_member_list_handler)),
    )
    .service(
      web::resource("{workspace_id}/collab_list").route(web::get().to(batch_get_collab_handler)),
    )
}

pub fn collab_scope() -> Scope {
  web::scope("/api/realtime").service(
    web::resource("post")
      .app_data(
        PayloadConfig::new(5 * 1024 * 1024), // 10 MB
      )
      .route(web::post().to(post_realtime_message_handler)),
  )
}

#[instrument(skip_all, err)]
async fn list_handler(
  uuid: UserUuid,
  state: Data<AppState>,
) -> Result<JsonAppResponse<AFWorkspaces>> {
  let rows = workspace::ops::get_all_user_workspaces(&state.pg_pool, &uuid).await?;
  let workspaces = rows
    .into_iter()
    .flat_map(|row| {
      let result = AFWorkspace::try_from(row);
      if let Err(err) = &result {
        event!(
          tracing::Level::ERROR,
          "Failed to convert workspace row to AFWorkspace: {:?}",
          err
        );
      }
      result
    })
    .collect::<Vec<_>>();
  Ok(AppResponse::Ok().with_data(AFWorkspaces(workspaces)).into())
}

#[instrument(skip(payload, state), err)]
async fn add_workspace_members_handler(
  user_uuid: UserUuid,
  workspace_id: web::Path<Uuid>,
  payload: Json<CreateWorkspaceMembers>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  let create_members = payload.into_inner();
  let role_by_uid = workspace::ops::add_workspace_members(
    &state.pg_pool,
    &user_uuid,
    &workspace_id,
    create_members.0,
  )
  .await?;

  for (uid, role) in role_by_uid {
    state
      .workspace_access_control
      .update_member(&uid, &workspace_id, role)
      .await?;
  }
  Ok(AppResponse::Ok().into())
}

#[instrument(skip_all, err)]
async fn get_workspace_members_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<Vec<AFWorkspaceMember>>> {
  let members = workspace::ops::get_workspace_members(&state.pg_pool, &user_uuid, &workspace_id)
    .await?
    .into_iter()
    .map(|member| AFWorkspaceMember {
      name: member.name,
      email: member.email,
      role: member.role,
      avatar_url: None,
    })
    .collect();

  Ok(AppResponse::Ok().with_data(members).into())
}

#[instrument(skip_all, err)]
async fn remove_workspace_member_handler(
  user_uuid: UserUuid,
  payload: Json<WorkspaceMembers>,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<()>> {
  let member_emails = payload
    .into_inner()
    .0
    .into_iter()
    .map(|member| member.0)
    .collect::<Vec<String>>();
  workspace::ops::remove_workspace_members(
    &user_uuid,
    &state.pg_pool,
    &workspace_id,
    &member_emails,
  )
  .await?;

  for email in member_emails {
    if let Ok(uid) = select_uid_from_email(&state.pg_pool, &email)
      .await
      .map_err(AppResponseError::from)
    {
      state
        .workspace_access_control
        .remove_member(&uid, &workspace_id)
        .await?;
    }
  }

  Ok(AppResponse::Ok().into())
}

#[instrument(level = "debug", skip_all, err)]
async fn open_workspace_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<AFWorkspace>> {
  let workspace_id = workspace_id.into_inner();
  let workspace = workspace::ops::open_workspace(&state.pg_pool, &user_uuid, &workspace_id).await?;
  Ok(AppResponse::Ok().with_data(workspace).into())
}

#[instrument(level = "debug", skip_all, err)]
async fn update_workspace_member_handler(
  payload: Json<WorkspaceMemberChangeset>,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<()>> {
  let workspace_id = workspace_id.into_inner();
  let changeset = payload.into_inner();
  workspace::ops::update_workspace_member(&state.pg_pool, &workspace_id, &changeset).await?;

  if let Some(role) = changeset.role {
    let uid = select_uid_from_email(&state.pg_pool, &changeset.email)
      .await
      .map_err(AppResponseError::from)?;
    state
      .workspace_access_control
      .update_member(&uid, &workspace_id, role)
      .await?;
  }

  Ok(AppResponse::Ok().into())
}

#[instrument(skip(state, payload), err)]
async fn create_collab_handler(
  user_uuid: UserUuid,
  payload: Json<InsertCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  biz::collab::ops::create_collab(
    &state.pg_pool,
    &user_uuid,
    &payload.into_inner(),
    &state.collab_access_control,
  )
  .await?;
  Ok(Json(AppResponse::Ok()))
}

#[instrument(level = "trace", skip(payload, state), err)]
async fn get_collab_handler(
  user_uuid: UserUuid,
  payload: Json<QueryCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<EncodedCollabV1>>> {
  let uid = select_uid_from_uuid(&state.pg_pool, &user_uuid)
    .await
    .map_err(AppResponseError::from)?;
  let data = state
    .collab_storage
    .get_collab_encoded_v1(&uid, payload.into_inner())
    .await
    .map_err(AppResponseError::from)?;

  Ok(Json(AppResponse::Ok().with_data(data)))
}

#[instrument(level = "trace", skip_all, err)]
async fn get_collab_snapshot_handler(
  payload: Json<QuerySnapshotParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<SnapshotData>>> {
  let data = state
    .collab_storage
    .get_collab_snapshot(&payload.snapshot_id)
    .await
    .map_err(AppResponseError::from)?;

  Ok(Json(AppResponse::Ok().with_data(data)))
}

#[instrument(level = "trace", skip_all, err)]
async fn create_collab_snapshot_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  path: web::Path<(String, String)>,
  payload: Json<CollabType>,
) -> Result<Json<AppResponse<AFSnapshotMeta>>> {
  let (workspace_id, object_id) = path.into_inner();
  let collab_type = payload.into_inner();
  let uid = select_uid_from_uuid(&state.pg_pool, &user_uuid)
    .await
    .map_err(AppResponseError::from)?;
  let encoded_collab_v1 = state
    .collab_storage
    .get_collab_encoded_v1(
      &uid,
      QueryCollabParams {
        collab_type,
        object_id: object_id.clone(),
        workspace_id: workspace_id.clone(),
      },
    )
    .await?
    .encode_to_bytes()
    .unwrap();

  let meta = state
    .collab_storage
    .create_snapshot(InsertSnapshotParams {
      object_id,
      workspace_id,
      encoded_collab_v1,
    })
    .await?;

  Ok(Json(AppResponse::Ok().with_data(meta)))
}

#[instrument(level = "trace", skip(path, state), err)]
async fn get_all_collab_snapshot_list_handler(
  path: web::Path<(String, String)>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<AFSnapshotMetas>>> {
  let (_, object_id) = path.into_inner();
  let data = state
    .collab_storage
    .get_collab_snapshot_list(&object_id)
    .await
    .map_err(AppResponseError::from)?;
  Ok(Json(AppResponse::Ok().with_data(data)))
}

#[instrument(level = "debug", skip(payload, state), err)]
async fn batch_get_collab_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  payload: Json<BatchQueryCollabParams>,
) -> Result<Json<AppResponse<BatchQueryCollabResult>>> {
  let uid = select_uid_from_uuid(&state.pg_pool, &user_uuid)
    .await
    .map_err(AppResponseError::from)?;
  let result = BatchQueryCollabResult(
    state
      .collab_storage
      .batch_get_collab(&uid, payload.into_inner().0)
      .await,
  );
  Ok(Json(AppResponse::Ok().with_data(result)))
}

#[instrument(skip(state, payload), err)]
async fn update_collab_handler(
  user_uuid: UserUuid,
  payload: Json<InsertCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  biz::collab::ops::upsert_collab(&state.pg_pool, &user_uuid, &payload.into_inner()).await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(level = "info", skip(state, payload), err)]
async fn delete_collab_handler(
  user_uuid: UserUuid,
  payload: Json<DeleteCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  biz::collab::ops::delete_collab(&state.pg_pool, &user_uuid, &payload.into_inner()).await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(level = "debug", skip(state, payload), err)]
async fn add_collab_member_handler(
  payload: Json<InsertCollabMemberParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  let payload = payload.into_inner();
  biz::collab::ops::create_collab_member(&state.pg_pool, &payload).await?;
  state
    .collab_access_control
    .update_member(&payload.uid, &payload.object_id, payload.access_level)
    .await;
  Ok(Json(AppResponse::Ok()))
}

#[instrument(level = "debug", skip(state, payload), err)]
async fn update_collab_member_handler(
  user_uuid: UserUuid,
  payload: Json<UpdateCollabMemberParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  let payload = payload.into_inner();
  biz::collab::ops::upsert_collab_member(&state.pg_pool, &user_uuid, &payload).await?;

  state
    .collab_access_control
    .update_member(&payload.uid, &payload.object_id, payload.access_level)
    .await;

  Ok(Json(AppResponse::Ok()))
}
#[instrument(level = "debug", skip(state, payload), err)]
async fn get_collab_member_handler(
  payload: Json<CollabMemberIdentify>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<AFCollabMember>>> {
  let payload = payload.into_inner();
  let member = biz::collab::ops::get_collab_member(&state.pg_pool, &payload).await?;
  Ok(Json(AppResponse::Ok().with_data(member)))
}

#[instrument(skip(state, payload), err)]
async fn remove_collab_member_handler(
  payload: Json<CollabMemberIdentify>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  let payload = payload.into_inner();
  biz::collab::ops::delete_collab_member(&state.pg_pool, &payload).await?;
  state
    .collab_access_control
    .remove_member(&payload.uid, &payload.object_id)
    .await;

  Ok(Json(AppResponse::Ok()))
}

#[instrument(level = "debug", skip(state, payload), err)]
async fn get_collab_member_list_handler(
  payload: Json<QueryCollabMembers>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<AFCollabMembers>>> {
  let members =
    biz::collab::ops::get_collab_member_list(&state.pg_pool, &payload.into_inner()).await?;
  Ok(Json(AppResponse::Ok().with_data(AFCollabMembers(members))))
}

#[instrument(level = "debug", skip(payload, server, state), err)]
async fn post_realtime_message_handler(
  user_uuid: UserUuid,
  payload: Bytes,
  server: Data<CollabServerImpl>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  let uid = select_uid_from_uuid(&state.pg_pool, &user_uuid)
    .await
    .map_err(AppResponseError::from)?;

  event!(
    tracing::Level::DEBUG,
    "Receive realtime message with len: {}",
    payload.len()
  );

  let HttpRealtimeMessage { device_id, payload } =
    HttpRealtimeMessage::decode(payload.as_ref()).map_err(|err| AppError::Internal(err.into()))?;

  let message = Message::from(payload);
  match message {
    Message::Binary(bytes) => {
      let realtime_msg = RealtimeMessage::try_from(bytes).map_err(|err| {
        AppError::InvalidRequest(format!("Failed to parse RealtimeMessage: {}", err))
      })?;

      match &realtime_msg {
        RealtimeMessage::Collab(msg) => {
          if !state
            .collab_access_control
            .can_send_collab_update(&uid, msg.object_id())
            .await?
          {
            return Err(
              AppError::NotEnoughPermissions(format!(
                "User {} is not allowed to edit: {}",
                uid,
                msg.object_id()
              ))
              .into(),
            );
          }
        },
        _ => {
          return Err(
            AppError::InvalidRequest(format!("Unsupported realtime message: {}", realtime_msg))
              .into(),
          );
        },
      }

      let realtime_user = Arc::new(RealtimeUserImpl::new(uid, device_id));
      server
        .send(ClientMessage {
          user: realtime_user,
          message: realtime_msg,
        })
        .await
        .map_err(|err| AppError::Unhandled(err.to_string()))?
        .map_err(|err| AppError::Internal(anyhow::Error::from(err)))?;
      Ok(Json(AppResponse::Ok()))
    },
    _ => Err(AppError::InvalidRequest(format!("Unsupported message type: {:?}", message)).into()),
  }
}
