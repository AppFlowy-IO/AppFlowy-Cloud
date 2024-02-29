use crate::api::util::{compress_type_from_header_value, device_id_from_headers};
use crate::api::ws::CollabServerImpl;
use crate::biz;
use crate::biz::workspace;
use crate::biz::workspace::access_control::WorkspaceAccessControl;
use crate::component::auth::jwt::UserUuid;
use crate::domain::compression::{decompress, CompressionType, X_COMPRESSION_TYPE};
use crate::state::AppState;

use std::time::Duration;

use actix_web::web::{Bytes, Payload};
use actix_web::web::{Data, Json, PayloadConfig};
use actix_web::{web, Scope};
use actix_web::{HttpRequest, Result};
use anyhow::{anyhow, Context};
use app_error::AppError;
use collab::core::collab_plugin::EncodedCollab;
use collab_entity::CollabType;
use database::collab::CollabStorage;
use database::user::select_uid_from_email;
use database_entity::dto::*;
use prost::Message as ProstMessage;

use bytes::BytesMut;
use realtime::entities::{ClientStreamMessage, RealtimeMessage};
use realtime_entity::realtime_proto::HttpRealtimeMessage;

use shared_entity::dto::workspace_dto::*;
use shared_entity::response::AppResponseError;
use shared_entity::response::{AppResponse, JsonAppResponse};
use sqlx::types::uuid;
use tokio::time::{sleep, Instant};

use tokio_stream::StreamExt;
use tokio_tungstenite::tungstenite::Message;
use tracing::{event, instrument};
use uuid::Uuid;
use validator::Validate;

pub const WORKSPACE_ID_PATH: &str = "workspace_id";
pub const COLLAB_OBJECT_ID_PATH: &str = "object_id";

pub fn workspace_scope() -> Scope {
  web::scope("/api/workspace")

    // deprecated, use the api below instead
    .service(web::resource("/list").route(web::get().to(list_workspace_handler)))

    .service(web::resource("")
      .route(web::get().to(list_workspace_handler))
      .route(web::post().to(create_workpace_handler))
      .route(web::patch().to(patch_workspace_handler))
    )
    .service(web::resource("/{workspace_id}")
      .route(web::delete().to(delete_workspace_handler))
    )

    .service(web::resource("/{workspace_id}/open").route(web::put().to(open_workspace_handler)))
    .service(
      web::resource("/{workspace_id}/member")
        .route(web::get().to(get_workspace_members_handler))
        .route(web::post().to(add_workspace_members_handler))
        .route(web::put().to(update_workspace_member_handler))
        .route(web::delete().to(remove_workspace_member_handler)),
    )
    .service(
      web::resource("/{workspace_id}/collab/{object_id}")
        .app_data(
          PayloadConfig::new(5 * 1024 * 1024), // 5 MB
        )
        .route(web::post().to(create_collab_handler))
        .route(web::get().to(get_collab_handler))
        .route(web::put().to(update_collab_handler))
        .route(web::delete().to(delete_collab_handler)),
    )
    .service(
      web::resource("/{workspace_id}/batch/collab")
        .route(web::post().to(batch_create_collab_handler)),
    )
    // will be deprecated
    .service(
      web::resource("/{workspace_id}/collabs")
          .app_data(PayloadConfig::new(10 * 1024 * 1024))
          .route(web::post().to(create_collab_list_handler)),
    )
    .service(
      web::resource("/{workspace_id}/{object_id}/snapshot")
        .route(web::get().to(get_collab_snapshot_handler))
        .route(web::post().to(create_collab_snapshot_handler)),
    )
    .service(
      web::resource("/{workspace_id}/{object_id}/snapshot/list")
        .route(web::get().to(get_all_collab_snapshot_list_handler)),
    )
    .service(
      web::resource("/{workspace_id}/collab/{object_id}/member")
        .route(web::post().to(add_collab_member_handler))
        .route(web::get().to(get_collab_member_handler))
        .route(web::put().to(update_collab_member_handler))
        .route(web::delete().to(remove_collab_member_handler)),
    )
    .service(
      web::resource("/{workspace_id}/collab/{object_id}/member/list")
        .route(web::get().to(get_collab_member_list_handler)),
    )
    .service(
      web::resource("/{workspace_id}/collab_list").route(web::get().to(batch_get_collab_handler)),
    )
}

pub fn collab_scope() -> Scope {
  web::scope("/api/realtime").service(
    web::resource("post/stream")
      .app_data(
        PayloadConfig::new(10 * 1024 * 1024), // 10 MB
      )
      .route(web::post().to(post_realtime_message_stream_handler)),
  )
}

// Adds a workspace for user, if success, return the workspace id
#[instrument(skip_all, err)]
async fn create_workpace_handler(
  uuid: UserUuid,
  state: Data<AppState>,
  create_workspace_param: Json<CreateWorkspaceParam>,
) -> Result<Json<AppResponse<AFWorkspace>>> {
  let workspace_name = create_workspace_param
    .into_inner()
    .workspace_name
    .unwrap_or_else(|| format!("workspace_{}", chrono::Utc::now().timestamp()));
  let new_workspace =
    workspace::ops::create_workspace_for_user(&state.pg_pool, &uuid, &workspace_name).await?;

  let uid = state.users.get_user_uid(&uuid).await?;
  state
    .workspace_access_control
    .insert_workspace_role(&uid, &new_workspace.workspace_id, AFRole::Owner)
    .await?;
  Ok(AppResponse::Ok().with_data(new_workspace).into())
}

// Adds a workspace for user, if success, return the workspace id
#[instrument(skip_all, err)]
async fn patch_workspace_handler(
  _uuid: UserUuid,
  state: Data<AppState>,
  params: Json<PatchWorkspaceParam>,
) -> Result<Json<AppResponse<()>>> {
  let params = params.into_inner();
  workspace::ops::patch_workspace(
    &state.pg_pool,
    &params.workspace_id,
    params.workspace_name.as_deref(),
    params.workspace_icon.as_deref(),
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

async fn delete_workspace_handler(
  _user_id: UserUuid,
  workspace_id: web::Path<Uuid>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  let bucket_storage = &state.bucket_storage;

  // TODO: add permission for workspace deletion
  workspace::ops::delete_workspace_for_user(&state.pg_pool, &workspace_id, bucket_storage).await?;
  Ok(AppResponse::Ok().into())
}

// TODO: also get shared workspaces
#[instrument(skip_all, err)]
async fn list_workspace_handler(
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
      .insert_workspace_role(&uid, &workspace_id, role)
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
        .remove_role(&uid, &workspace_id)
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
      .insert_workspace_role(&uid, &workspace_id, role)
      .await?;
  }

  Ok(AppResponse::Ok().into())
}

#[instrument(skip(state, payload), err)]
async fn create_collab_handler(
  user_uuid: UserUuid,
  payload: Bytes,
  state: Data<AppState>,
  req: HttpRequest,
) -> Result<Json<AppResponse<()>>> {
  let uid = state.users.get_user_uid(&user_uuid).await?;
  let params = match req.headers().get(X_COMPRESSION_TYPE) {
    None => serde_json::from_slice::<CreateCollabParams>(&payload).map_err(|err| {
      AppError::InvalidRequest(format!(
        "Failed to parse CreateCollabParams from JSON: {}",
        err
      ))
    })?,
    Some(_) => match compress_type_from_header_value(req.headers())? {
      CompressionType::Brotli { buffer_size } => {
        let decompress_data = decompress(payload.to_vec(), buffer_size).await?;
        CreateCollabParams::from_bytes(&decompress_data).map_err(|err| {
          AppError::InvalidRequest(format!(
            "Failed to parse CreateCollabParams with brotli decompression data: {}",
            err
          ))
        })?
      },
    },
  };

  params.validate().map_err(AppError::from)?;
  let object_id = params.object_id.clone();
  state.collab_storage.upsert_collab(&uid, params).await?;
  state
    .collab_access_control
    .update_member(&uid, &object_id, AFAccessLevel::FullAccess)
    .await;

  Ok(Json(AppResponse::Ok()))
}

#[instrument(skip(state, payload), err)]
async fn batch_create_collab_handler(
  user_uuid: UserUuid,
  workspace_id: web::Path<Uuid>,
  mut payload: Payload,
  state: Data<AppState>,
  req: HttpRequest,
) -> Result<Json<AppResponse<()>>> {
  let uid = state.users.get_user_uid(&user_uuid).await?;
  let mut collab_params_list = vec![];
  let workspace_id = workspace_id.into_inner().to_string();
  let compress_type = compress_type_from_header_value(req.headers())?;
  event!(
    tracing::Level::DEBUG,
    "start decompressing collab params list"
  );

  let start_time = Instant::now();
  let mut payload_buffer = Vec::new();
  while let Some(item) = payload.next().await {
    if let Ok(bytes) = item {
      match compress_type {
        CompressionType::Brotli { buffer_size } => {
          payload_buffer.extend_from_slice(&bytes);

          // The client API uses a u32 value as the frame separator, which determines the size of each data frame.
          // The length of a u32 is fixed at 4 bytes. It's important not to change the size (length) of the u32,
          // unless you also make a corresponding update in the client API. Any mismatch in frame size handling
          // between the client and server could lead to incorrect data processing or communication errors.
          while payload_buffer.len() >= 4 {
            let size = u32::from_be_bytes([
              payload_buffer[0],
              payload_buffer[1],
              payload_buffer[2],
              payload_buffer[3],
            ]) as usize;

            if payload_buffer.len() < 4 + size {
              break;
            }

            let compressed_data = payload_buffer[4..4 + size].to_vec();
            let decompress_data = decompress(compressed_data, buffer_size).await?;
            let params = CollabParams::from_bytes(&decompress_data).map_err(|err| {
              AppError::InvalidRequest(format!(
                "Failed to parse CollabParams with brotli decompression data: {}",
                err
              ))
            })?;
            params.validate().map_err(AppError::from)?;
            collab_params_list.push(params);

            payload_buffer = payload_buffer[4 + size..].to_vec();
          }
        },
      }
    }
  }
  let duration = start_time.elapsed();
  event!(
    tracing::Level::DEBUG,
    "end decompressing collab params list, time taken: {:?}",
    duration
  );

  if collab_params_list.is_empty() {
    return Err(AppError::InvalidRequest("Empty collab params list".to_string()).into());
  }
  let mut transaction = state
    .pg_pool
    .begin()
    .await
    .context("acquire transaction to upsert collab")
    .map_err(AppError::from)?;
  for params in collab_params_list {
    let object_id = params.object_id.clone();
    state
      .collab_storage
      .upsert_collab_with_transaction(&workspace_id, &uid, params, &mut transaction)
      .await?;

    state
      .collab_access_control
      .update_member(&uid, &object_id, AFAccessLevel::FullAccess)
      .await;
  }

  transaction
    .commit()
    .await
    .context("fail to commit the transaction to upsert collab")
    .map_err(AppError::from)?;
  Ok(Json(AppResponse::Ok()))
}

#[instrument(skip(state, payload), err)]
async fn create_collab_list_handler(
  user_uuid: UserUuid,
  payload: Bytes,
  state: Data<AppState>,
  req: HttpRequest,
) -> Result<Json<AppResponse<()>>> {
  let uid = state.users.get_user_uid(&user_uuid).await?;
  let params = match req.headers().get(X_COMPRESSION_TYPE) {
    None => BatchCreateCollabParams::from_bytes(&payload).map_err(|err| {
      AppError::InvalidRequest(format!(
        "Failed to parse batch BatchCreateCollabParams: {}",
        err
      ))
    })?,
    Some(_) => match compress_type_from_header_value(req.headers())? {
      CompressionType::Brotli { buffer_size } => {
        let decompress_data = decompress(payload.to_vec(), buffer_size).await?;
        BatchCreateCollabParams::from_bytes(&decompress_data).map_err(|err| {
          AppError::InvalidRequest(format!(
            "Failed to parse BatchCreateCollabParams with decompression data: {}",
            err
          ))
        })?
      },
    },
  };

  params.validate().map_err(AppError::from)?;
  let BatchCreateCollabParams {
    workspace_id,
    params_list,
  } = params;

  if params_list.is_empty() {
    return Err(AppError::InvalidRequest("Empty collab params list".to_string()).into());
  }

  let mut transaction = state
    .pg_pool
    .begin()
    .await
    .context("acquire transaction to upsert collab")
    .map_err(AppError::from)?;

  for params in params_list {
    let object_id = params.object_id.clone();
    state
      .collab_storage
      .upsert_collab_with_transaction(&workspace_id, &uid, params, &mut transaction)
      .await?;

    state
      .collab_access_control
      .update_member(&uid, &object_id, AFAccessLevel::FullAccess)
      .await;
  }

  transaction
    .commit()
    .await
    .context("fail to commit the transaction to upsert collab")
    .map_err(AppError::from)?;

  Ok(Json(AppResponse::Ok()))
}
async fn get_collab_handler(
  user_uuid: UserUuid,
  payload: Json<QueryCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<EncodedCollab>>> {
  let uid = state
    .users
    .get_user_uid(&user_uuid)
    .await
    .map_err(AppResponseError::from)?;
  let data = state
    .collab_storage
    .get_collab_encoded(&uid, payload.into_inner(), false)
    .await
    .map_err(AppResponseError::from)?;

  Ok(Json(AppResponse::Ok().with_data(data)))
}

#[instrument(level = "trace", skip_all, err)]
async fn get_collab_snapshot_handler(
  payload: Json<QuerySnapshotParams>,
  path: web::Path<(String, String)>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<SnapshotData>>> {
  let (workspace_id, object_id) = path.into_inner();
  let data = state
    .collab_storage
    .get_collab_snapshot(&workspace_id.to_string(), &object_id, &payload.snapshot_id)
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
  let uid = state
    .users
    .get_user_uid(&user_uuid)
    .await
    .map_err(AppResponseError::from)?;
  let encoded_collab_v1 = state
    .collab_storage
    .get_collab_encoded(
      &uid,
      QueryCollabParams::new(&object_id, collab_type, &workspace_id),
      false,
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
  let uid = state
    .users
    .get_user_uid(&user_uuid)
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
  payload: Json<CreateCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  let (params, workspace_id) = payload.into_inner().split();
  let uid = state.users.get_user_uid(&user_uuid).await?;

  let create_params = CreateCollabParams::from((workspace_id.to_string(), params));
  state
    .collab_storage
    .upsert_collab(&uid, create_params)
    .await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(level = "info", skip(state, payload), err)]
async fn delete_collab_handler(
  user_uuid: UserUuid,
  payload: Json<DeleteCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  let payload = payload.into_inner();
  payload.validate().map_err(AppError::from)?;

  let uid = state
    .users
    .get_user_uid(&user_uuid)
    .await
    .map_err(AppResponseError::from)?;

  state
    .collab_storage
    .delete_collab(&uid, &payload.object_id)
    .await
    .map_err(AppResponseError::from)?;

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

#[instrument(level = "info", skip_all, err)]
async fn post_realtime_message_stream_handler(
  user_uuid: UserUuid,
  mut payload: Payload,
  server: Data<CollabServerImpl>,
  state: Data<AppState>,
  req: HttpRequest,
) -> Result<Json<AppResponse<()>>> {
  // TODO(nathan): after upgrade the client application, then the device_id should not be empty
  let device_id = device_id_from_headers(req.headers()).unwrap_or_else(|_| "".to_string());
  let uid = state
    .users
    .get_user_uid(&user_uuid)
    .await
    .map_err(AppResponseError::from)?;

  let mut bytes = BytesMut::new();
  while let Some(item) = payload.next().await {
    bytes.extend_from_slice(&item?);
  }

  event!(tracing::Level::INFO, "message len: {}", bytes.len());
  let device_id = device_id.to_string();
  let message = parser_realtime_msg(bytes.freeze(), req.clone()).await?;
  let stream = Box::new(tokio_stream::once(message));
  let mut stream_message = Some(ClientStreamMessage {
    uid,
    device_id,
    stream,
  });
  const MAX_RETRIES: usize = 3;
  const RETRY_DELAY: Duration = Duration::from_secs(2);

  let mut attempts = 0;
  while attempts < MAX_RETRIES {
    match stream_message.take() {
      None => {
        return Err(AppError::Internal(anyhow!("Unexpected empty stream message")).into());
      },
      Some(message_to_send) => {
        match server.try_send(message_to_send) {
          Ok(_) => return Ok(Json(AppResponse::Ok())),
          Err(err) if attempts < MAX_RETRIES - 1 => {
            attempts += 1;
            stream_message = Some(err.into_inner());
            sleep(RETRY_DELAY).await;
          },
          Err(err) => {
            return Err(
              AppError::Internal(anyhow!(
                "Failed to send client stream message to websocket server after {} attempts: {}",
                // attempts starts from 0, so add 1 for accurate count
                attempts + 1,
                err
              ))
              .into(),
            );
          },
        }
      },
    }
  }
  Err(AppError::Internal(anyhow!("Failed to send message to websocket server")).into())
}

#[inline]
async fn parser_realtime_msg(
  payload: Bytes,
  req: HttpRequest,
) -> Result<RealtimeMessage, AppError> {
  let HttpRealtimeMessage {
    device_id: _,
    payload,
  } =
    HttpRealtimeMessage::decode(payload.as_ref()).map_err(|err| AppError::Internal(err.into()))?;
  let payload = match req.headers().get(X_COMPRESSION_TYPE) {
    None => payload,
    Some(_) => match compress_type_from_header_value(req.headers())? {
      CompressionType::Brotli { buffer_size } => {
        let decompressed_data = decompress(payload, buffer_size).await?;
        event!(
          tracing::Level::TRACE,
          "Decompress realtime http message with len: {}",
          decompressed_data.len()
        );
        decompressed_data
      },
    },
  };
  let message = Message::from(payload);
  match message {
    Message::Binary(bytes) => {
      let realtime_msg = tokio::task::spawn_blocking(move || {
        RealtimeMessage::try_from(bytes).map_err(|err| {
          AppError::InvalidRequest(format!("Failed to parse RealtimeMessage: {}", err))
        })
      })
      .await
      .map_err(AppError::from)??;
      Ok(realtime_msg)
    },
    _ => Err(AppError::InvalidRequest(format!(
      "Unsupported message type: {:?}",
      message
    ))),
  }
}
