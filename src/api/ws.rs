use crate::state::AppState;
use actix::Addr;
use actix_web::web::{Data, Path, Payload};
use actix_web::{get, web, HttpRequest, HttpResponse, Result, Scope};
use actix_web_actors::ws;
use std::sync::Arc;

use crate::biz::collab::storage::CollabAccessControlStorage;
use crate::biz::user::RealtimeUserImpl;
use crate::component::auth::jwt::{authorization_from_token, UserUuid};

use crate::biz::casbin::RealtimeCollabAccessControlImpl;
use actix_http::header::AUTHORIZATION;
use app_error::AppError;
use realtime::client::rt_client::RealtimeClient;
use realtime::server::RealtimeServer;

use semver::Version;
use shared_entity::response::AppResponseError;
use std::time::Duration;
use tracing::{debug, error, instrument};

pub fn ws_scope() -> Scope {
  web::scope("/ws")
    .service(establish_ws_connection)
    .service(web::resource("/v1").route(web::get().to(establish_ws_connection_v1)))
}
const MAX_FRAME_SIZE: usize = 65_536; // 64 KiB

pub type RealtimeServerAddr = Addr<
  RealtimeServer<
    CollabAccessControlStorage,
    Arc<RealtimeUserImpl>,
    RealtimeCollabAccessControlImpl,
  >,
>;

/// This function will not be used after the 0.5.0 of the client.
#[instrument(skip_all, err)]
#[get("/{token}/{device_id}")]
pub async fn establish_ws_connection(
  request: HttpRequest,
  payload: Payload,
  path: Path<(String, String)>,
  state: Data<AppState>,
  server: Data<RealtimeServerAddr>,
) -> Result<HttpResponse> {
  let (access_token, device_id) = path.into_inner();
  let client_version = Version::new(0, 5, 0);
  start_connect(
    &request,
    payload,
    &state,
    server,
    access_token,
    device_id,
    client_version,
  )
  .await
}

#[instrument(skip_all, err)]
pub async fn establish_ws_connection_v1(
  request: HttpRequest,
  payload: Payload,
  state: Data<AppState>,
  server: Data<RealtimeServerAddr>,
) -> Result<HttpResponse> {
  let ConnectInfo {
    access_token,
    client_version,
    device_id,
  } = ConnectInfo::try_from(&request)?;

  start_connect(
    &request,
    payload,
    &state,
    server,
    access_token,
    device_id,
    client_version,
  )
  .await
}

#[inline]
async fn start_connect(
  request: &HttpRequest,
  payload: Payload,
  state: &Data<AppState>,
  server: Data<RealtimeServerAddr>,
  access_token: String,
  device_id: String,
  client_version: Version,
) -> Result<HttpResponse> {
  let auth = authorization_from_token(access_token.as_str(), state)?;
  let user_uuid = UserUuid::from_auth(auth)?;
  let result = state.user_cache.get_user_uid(&user_uuid).await;

  match result {
    Ok(uid) => {
      let user_change_recv = state.pg_listeners.subscribe_user_change(uid);
      debug!(
        "🚀new websocket connect: uid={}, device_id={}, client_version:{}",
        uid, device_id, client_version
      );

      let realtime_user = Arc::new(RealtimeUserImpl::new(uid, device_id));
      let client = RealtimeClient::new(
        realtime_user,
        user_change_recv,
        server.get_ref().clone(),
        Duration::from_secs(state.config.websocket.heartbeat_interval as u64),
        Duration::from_secs(state.config.websocket.client_timeout as u64),
        client_version,
      );

      match ws::WsResponseBuilder::new(client, request, payload)
        .frame_size(MAX_FRAME_SIZE * 2)
        .start()
      {
        Ok(response) => Ok(response),
        Err(e) => {
          tracing::error!("🔴ws connection error: {:?}", e);
          Err(e)
        },
      }
    },
    Err(err) => {
      if err.is_record_not_found() {
        return Ok(HttpResponse::NotFound().json("user not found"));
      }
      Err(AppResponseError::from(err).into())
    },
  }
}

struct ConnectInfo {
  access_token: String,
  client_version: Version,
  device_id: String,
}

impl TryFrom<&HttpRequest> for ConnectInfo {
  type Error = AppError;
  fn try_from(req: &HttpRequest) -> Result<Self, Self::Error> {
    let headers = req.headers();

    let access_token = match headers.get(AUTHORIZATION) {
      Some(token) => token
        .to_str()
        .map_err(|_| AppError::InvalidRequest("invalid access token".to_string()))?,
      None => return Err(AppError::OAuthError("no access token".to_string())),
    };

    let client_version = headers
      .get("client-version")
      .map(|v| {
        v.to_str()
          .map_err(|_| AppError::InvalidRequest("invalid client version".to_string()))
          .and_then(|v| {
            Version::parse(v)
              .map_err(|_| AppError::InvalidRequest("fail to parse client version".to_string()))
          })
      })
      .unwrap_or_else(|| {
        error!("fail to get client version from header, use default version 0.5.0");
        Ok(Version::new(0, 5, 0))
      })?;

    let device_id = match headers.get("device-id") {
      Some(device_id) => device_id
        .to_str()
        .map_err(|_| AppError::InvalidRequest("invalid device id".to_string()))?,
      None => return Err(AppError::InvalidRequest("empty device id".to_string())),
    };

    Ok(Self {
      access_token: access_token.to_string(),
      client_version,
      device_id: device_id.to_string(),
    })
  }
}
