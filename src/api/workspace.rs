use actix_web::web::{Bytes, Payload};
use actix_web::web::{Data, Json, PayloadConfig};
use actix_web::{web, Scope};
use actix_web::{HttpRequest, Result};
use anyhow::{anyhow, Context};
use bytes::BytesMut;
use collab_entity::CollabType;
use prost::Message as ProstMessage;
use sqlx::types::uuid;
use tokio::time::Instant;
use tokio_stream::StreamExt;
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, event, instrument};
use uuid::Uuid;
use validator::Validate;

use access_control::collab::CollabAccessControl;
use app_error::AppError;
use appflowy_collaborate::actix_ws::entities::ClientStreamMessage;
use collab_rt_entity::realtime_proto::HttpRealtimeMessage;
use collab_rt_entity::RealtimeMessage;
use collab_rt_protocol::validate_encode_collab;
use database::collab::CollabStorage;
use database::user::select_uid_from_email;
use database_entity::dto::*;
use shared_entity::dto::workspace_dto::*;
use shared_entity::response::AppResponseError;
use shared_entity::response::{AppResponse, JsonAppResponse};

use crate::api::util::{compress_type_from_header_value, device_id_from_headers, CollabValidator};
use crate::api::ws::RealtimeServerAddr;
use crate::biz;
use crate::biz::user::auth::jwt::UserUuid;
use crate::biz::workspace;
use crate::domain::compression::{decompress, CompressionType, X_COMPRESSION_TYPE};
use crate::state::AppState;

pub const WORKSPACE_ID_PATH: &str = "workspace_id";
pub const COLLAB_OBJECT_ID_PATH: &str = "object_id";

pub const WORKSPACE_PATTERN: &str = "/api/workspace";
pub const WORKSPACE_MEMBER_PATTERN: &str = "/api/workspace/{workspace_id}/member";
pub const WORKSPACE_INVITE_PATTERN: &str = "/api/workspace/{workspace_id}/invite";
pub const COLLAB_PATTERN: &str = "/api/workspace/{workspace_id}/collab/{object_id}";
pub const V1_COLLAB_PATTERN: &str = "/api/workspace/v1/{workspace_id}/collab/{object_id}";

pub fn workspace_scope() -> Scope {
  web::scope("/api/workspace")
    // deprecated, use the api below instead
    .service(web::resource("/list").route(web::get().to(list_workspace_handler)))

    .service(web::resource("")
      .route(web::get().to(list_workspace_handler))
      .route(web::post().to(create_workspace_handler))
      .route(web::patch().to(patch_workspace_handler))
    )
    .service(
      web::resource("/{workspace_id}/invite")
        .route(web::post().to(post_workspace_invite_handler)) // invite members to workspace
    )
    .service(
      web::resource("/invite")
        .route(web::get().to(get_workspace_invite_handler)) // show invites for user
    )
    .service(
      web::resource("/accept-invite/{invite_id}")
        .route(web::post().to(post_accept_workspace_invite_handler)) // accept invitation to workspace
    )
    .service(web::resource("/{workspace_id}")
      .route(web::delete().to(delete_workspace_handler))
    )
    .service(web::resource("/{workspace_id}/open").route(web::put().to(open_workspace_handler)))
    .service(web::resource("/{workspace_id}/leave").route(web::post().to(leave_workspace_handler)))
    .service(
      web::resource("/{workspace_id}/member")
        .route(web::get().to(get_workspace_members_handler))
        .route(web::post().to(create_workspace_members_handler)) // deprecated, use invite flow instead
        .route(web::put().to(update_workspace_member_handler))
        .route(web::delete().to(remove_workspace_member_handler))
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
      web::resource("/v1/{workspace_id}/collab/{object_id}")
        .route(web::get().to(v1_get_collab_handler))
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
      web::resource("/{workspace_id}/usage").route(web::get().to(get_workspace_usage_handler)),
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
      web::resource("/{workspace_id}/collab_list")
      .route(web::get().to(batch_get_collab_handler)) // deprecated: browser cannot use json param
                                                      // for GET request
      .route(web::post().to(batch_get_collab_handler)),
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
async fn create_workspace_handler(
  uuid: UserUuid,
  state: Data<AppState>,
  create_workspace_param: Json<CreateWorkspaceParam>,
) -> Result<Json<AppResponse<AFWorkspace>>> {
  let workspace_name = create_workspace_param
    .into_inner()
    .workspace_name
    .unwrap_or_else(|| format!("workspace_{}", chrono::Utc::now().timestamp()));

  let uid = state.user_cache.get_user_uid(&uuid).await?;
  let new_workspace = workspace::ops::create_workspace_for_user(
    &state.pg_pool,
    &state.workspace_access_control,
    &state.collab_access_control_storage,
    &uuid,
    uid,
    &workspace_name,
  )
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

// Deprecated
#[instrument(skip(payload, state), err)]
async fn create_workspace_members_handler(
  user_uuid: UserUuid,
  workspace_id: web::Path<Uuid>,
  payload: Json<CreateWorkspaceMembers>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  let create_members = payload.into_inner();
  workspace::ops::add_workspace_members(
    &state.pg_pool,
    &user_uuid,
    &workspace_id,
    create_members.0,
    &state.workspace_access_control,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(skip(payload, state), err)]
async fn post_workspace_invite_handler(
  user_uuid: UserUuid,
  workspace_id: web::Path<Uuid>,
  payload: Json<Vec<WorkspaceMemberInvitation>>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  let invited_members = payload.into_inner();
  workspace::ops::invite_workspace_members(
    &state.mailer,
    &state.gotrue_admin,
    &state.pg_pool,
    &state.gotrue_client,
    &user_uuid,
    &workspace_id,
    invited_members,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

async fn get_workspace_invite_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  query: web::Query<WorkspaceInviteQuery>,
) -> Result<JsonAppResponse<Vec<AFWorkspaceInvitation>>> {
  let query = query.into_inner();
  let res =
    workspace::ops::list_workspace_invitations_for_user(&state.pg_pool, &user_uuid, query.status)
      .await?;
  Ok(AppResponse::Ok().with_data(res).into())
}

async fn post_accept_workspace_invite_handler(
  user_uuid: UserUuid,
  invite_id: web::Path<Uuid>,
  state: Data<AppState>,
) -> Result<JsonAppResponse<()>> {
  let invite_id = invite_id.into_inner();
  // TODO(zack): insert a workspace member in the af_workspace_member by calling  workspace::ops::add_workspace_members.
  // Currently, when the server get restarted, the policy in access control will be lost.
  workspace::ops::accept_workspace_invite(
    &state.pg_pool,
    &state.workspace_access_control,
    &user_uuid,
    &invite_id,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(skip_all, err)]
async fn get_workspace_members_handler(
  _user_uuid: UserUuid,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<Vec<AFWorkspaceMember>>> {
  let members = workspace::ops::get_workspace_members(&state.pg_pool, &workspace_id)
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
  _user_uuid: UserUuid,
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
    &state.pg_pool,
    &workspace_id,
    &member_emails,
    &state.workspace_access_control,
  )
  .await?;

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
async fn leave_workspace_handler(
  user_uuid: UserUuid,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<()>> {
  let workspace_id = workspace_id.into_inner();
  workspace::ops::leave_workspace(
    &state.pg_pool,
    &workspace_id,
    &user_uuid,
    &state.workspace_access_control,
  )
  .await?;
  Ok(AppResponse::Ok().into())
}

#[instrument(level = "debug", skip_all, err)]
async fn update_workspace_member_handler(
  payload: Json<WorkspaceMemberChangeset>,
  state: Data<AppState>,
  workspace_id: web::Path<Uuid>,
) -> Result<JsonAppResponse<()>> {
  // TODO: only owner is allowed to update member role

  let workspace_id = workspace_id.into_inner();
  let changeset = payload.into_inner();

  if changeset.role.is_some() {
    let uid = select_uid_from_email(&state.pg_pool, &changeset.email)
      .await
      .map_err(AppResponseError::from)?;
    workspace::ops::update_workspace_member(
      &uid,
      &state.pg_pool,
      &workspace_id,
      &changeset,
      &state.workspace_access_control,
    )
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
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
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

  let (params, workspace_id) = params.split();

  if params.object_id == workspace_id {
    // Only the object with [CollabType::Folder] can have the same object_id as workspace_id. But
    // it should use create workspace API
    return Err(
      AppError::InvalidRequest("object_id cannot be the same as workspace_id".to_string()).into(),
    );
  }

  if let Err(err) = params.check_encode_collab().await {
    return Err(
      AppError::NoRequiredData(format!(
        "collab doc state is not correct:{},{}",
        params.object_id, err
      ))
      .into(),
    );
  }

  let mut transaction = state
    .pg_pool
    .begin()
    .await
    .context("acquire transaction to upsert collab")
    .map_err(AppError::from)?;
  state
    .collab_access_control_storage
    .insert_new_collab_with_transaction(&workspace_id, &uid, params, &mut transaction)
    .await?;

  transaction
    .commit()
    .await
    .context("fail to commit the transaction to upsert collab")
    .map_err(AppError::from)?;

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
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
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
            match params.check_encode_collab().await {
              Ok(_) => collab_params_list.push(params),
              Err(err) => error!("Failed to validate collab params: {:?}", err),
            }

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
  for params in collab_params_list {
    let object_id = params.object_id.clone();
    if validate_encode_collab(
      &params.object_id,
      &params.encoded_collab_v1,
      &params.collab_type,
    )
    .await
    .is_ok()
    {
      state
        .collab_access_control_storage
        .insert_new_collab(&workspace_id, &uid, params)
        .await?;

      state
        .collab_access_control
        .update_access_level_policy(&uid, &object_id, AFAccessLevel::FullAccess)
        .await?;
    }
  }
  Ok(Json(AppResponse::Ok()))
}

#[instrument(skip(state, payload), err)]
async fn create_collab_list_handler(
  user_uuid: UserUuid,
  payload: Bytes,
  state: Data<AppState>,
  req: HttpRequest,
) -> Result<Json<AppResponse<()>>> {
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;
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

  let mut valid_items = Vec::with_capacity(params_list.len());
  for params in params_list {
    match params.check_encode_collab().await {
      Ok(_) => valid_items.push(params),
      Err(err) => error!("Failed to validate collab params: {:?}", err),
    }
  }

  if valid_items.is_empty() {
    return Err(AppError::InvalidRequest("Empty collab params list".to_string()).into());
  }

  for params in valid_items {
    let _object_id = params.object_id.clone();
    state
      .collab_access_control_storage
      .insert_new_collab(&workspace_id, &uid, params)
      .await?;
  }

  Ok(Json(AppResponse::Ok()))
}

// Deprecated
async fn get_collab_handler(
  user_uuid: UserUuid,
  payload: Json<QueryCollabParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<CollabResponse>>> {
  let uid = state
    .user_cache
    .get_user_uid(&user_uuid)
    .await
    .map_err(AppResponseError::from)?;
  let params = payload.into_inner();
  let object_id = params.object_id.clone();
  let encode_collab = state
    .collab_access_control_storage
    .get_encode_collab(&uid, params, false)
    .await
    .map_err(AppResponseError::from)?;

  let resp = CollabResponse {
    encode_collab,
    object_id,
  };

  Ok(Json(AppResponse::Ok().with_data(resp)))
}

async fn v1_get_collab_handler(
  user_uuid: UserUuid,
  path: web::Path<(String, String)>,
  query: web::Query<CollabTypeParam>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<CollabResponse>>> {
  let (workspace_id, object_id) = path.into_inner();
  let collab_type = query.into_inner().collab_type;
  let uid = state
    .user_cache
    .get_user_uid(&user_uuid)
    .await
    .map_err(AppResponseError::from)?;

  let param = QueryCollabParams {
    workspace_id,
    inner: QueryCollab {
      object_id: object_id.clone(),
      collab_type,
    },
  };

  let encode_collab = state
    .collab_access_control_storage
    .get_encode_collab(&uid, param, false)
    .await
    .map_err(AppResponseError::from)?;

  let resp = CollabResponse {
    encode_collab,
    object_id,
  };

  Ok(Json(AppResponse::Ok().with_data(resp)))
}

#[instrument(level = "trace", skip_all, err)]
async fn get_collab_snapshot_handler(
  payload: Json<QuerySnapshotParams>,
  path: web::Path<(String, String)>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<SnapshotData>>> {
  let (workspace_id, object_id) = path.into_inner();
  let data = state
    .collab_access_control_storage
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
    .user_cache
    .get_user_uid(&user_uuid)
    .await
    .map_err(AppResponseError::from)?;
  let encoded_collab_v1 = state
    .collab_access_control_storage
    .get_encode_collab(
      &uid,
      QueryCollabParams::new(&object_id, collab_type.clone(), &workspace_id),
      false,
    )
    .await?
    .encode_to_bytes()
    .unwrap();

  let meta = state
    .collab_access_control_storage
    .create_snapshot(InsertSnapshotParams {
      object_id,
      workspace_id,
      encoded_collab_v1,
      collab_type,
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
    .collab_access_control_storage
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
    .user_cache
    .get_user_uid(&user_uuid)
    .await
    .map_err(AppResponseError::from)?;
  let result = BatchQueryCollabResult(
    state
      .collab_access_control_storage
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
  let uid = state.user_cache.get_user_uid(&user_uuid).await?;

  let create_params = CreateCollabParams::from((workspace_id.to_string(), params));
  let (params, workspace_id) = create_params.split();
  state
    .collab_access_control_storage
    .insert_or_update_collab(&workspace_id, &uid, params, false)
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
    .user_cache
    .get_user_uid(&user_uuid)
    .await
    .map_err(AppResponseError::from)?;

  state
    .collab_access_control_storage
    .delete_collab(&payload.workspace_id, &uid, &payload.object_id)
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
  if !state.collab_cache.is_exist(&payload.object_id).await? {
    return Err(
      AppError::RecordNotFound(format!(
        "Fail to insert collab member. The Collab with object_id {} does not exist",
        payload.object_id
      ))
      .into(),
    );
  }

  biz::collab::ops::create_collab_member(&state.pg_pool, &payload, &state.collab_access_control)
    .await?;
  Ok(Json(AppResponse::Ok()))
}

#[instrument(level = "debug", skip(state, payload), err)]
async fn update_collab_member_handler(
  user_uuid: UserUuid,
  payload: Json<UpdateCollabMemberParams>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<()>>> {
  let payload = payload.into_inner();

  if !state.collab_cache.is_exist(&payload.object_id).await? {
    return Err(
      AppError::RecordNotFound(format!(
        "Fail to update collab member. The Collab with object_id {} does not exist",
        payload.object_id
      ))
      .into(),
    );
  }
  biz::collab::ops::upsert_collab_member(
    &state.pg_pool,
    &user_uuid,
    &payload,
    &state.collab_access_control,
  )
  .await?;
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
  biz::collab::ops::delete_collab_member(&state.pg_pool, &payload, &state.collab_access_control)
    .await?;

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
  server: Data<RealtimeServerAddr>,
  state: Data<AppState>,
  req: HttpRequest,
) -> Result<Json<AppResponse<()>>> {
  // TODO(nathan): after upgrade the client application, then the device_id should not be empty
  let device_id = device_id_from_headers(req.headers()).unwrap_or_else(|_| "".to_string());
  let uid = state
    .user_cache
    .get_user_uid(&user_uuid)
    .await
    .map_err(AppResponseError::from)?;

  let mut bytes = BytesMut::new();
  while let Some(item) = payload.next().await {
    bytes.extend_from_slice(&item?);
  }

  event!(tracing::Level::INFO, "message len: {}", bytes.len());
  let device_id = device_id.to_string();
  // Only send message to websocket server when the user is connected
  if !state
    .realtime_shared_state
    .is_user_connected(&uid, &device_id)
    .await
    .unwrap_or(false)
  {
    return Ok(Json(AppResponse::Ok()));
  }

  let message = parser_realtime_msg(bytes.freeze(), req.clone()).await?;
  let stream_message = ClientStreamMessage {
    uid,
    device_id,
    message,
  };

  // When the server is under heavy load, try_send may fail. In client side, it will retry to send
  // the message later.
  match server.try_send(stream_message) {
    Ok(_) => return Ok(Json(AppResponse::Ok())),
    Err(err) => Err(
      AppError::Internal(anyhow!(
        "Failed to send message to websocket server, error:{}",
        err
      ))
      .into(),
    ),
  }
}

async fn get_workspace_usage_handler(
  user_uuid: UserUuid,
  workspace_id: web::Path<Uuid>,
  state: Data<AppState>,
) -> Result<Json<AppResponse<WorkspaceUsage>>> {
  let res = biz::workspace::ops::get_workspace_document_total_bytes(
    &state.pg_pool,
    &user_uuid,
    &workspace_id,
  )
  .await?;
  Ok(Json(AppResponse::Ok().with_data(res)))
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
        RealtimeMessage::decode(&bytes).map_err(|err| {
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
