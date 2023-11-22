use crate::biz::casbin::access_control::CasbinCollabAccessControl;
use crate::state::AppState;
use actix::Addr;
use actix_web::web::{Data, Path, Payload};
use actix_web::{get, web, HttpRequest, HttpResponse, Result, Scope};
use actix_web_actors::ws;
use std::sync::Arc;

use realtime::client::ClientSession;
use realtime::collaborate::CollabServer;

use crate::biz::collab::storage::CollabPostgresDBStorage;
use crate::biz::user::RealtimeUserImpl;
use crate::component::auth::jwt::{authorization_from_token, UserUuid};
use database::user::select_uid_from_uuid;
use shared_entity::response::AppResponseError;
use std::time::Duration;
use tracing::instrument;

pub fn ws_scope() -> Scope {
  web::scope("/ws").service(establish_ws_connection)
}
const MAX_FRAME_SIZE: usize = 65_536; // 64 KiB

pub type CollabServerImpl =
  Addr<CollabServer<CollabPostgresDBStorage, Arc<RealtimeUserImpl>, CasbinCollabAccessControl>>;

#[instrument(skip_all, err)]
#[get("/{token}/{device_id}")]
pub async fn establish_ws_connection(
  request: HttpRequest,
  payload: Payload,
  path: Path<(String, String)>,
  state: Data<AppState>,
  server: Data<CollabServerImpl>,
) -> Result<HttpResponse> {
  tracing::info!("receive ws connect: {:?}", request);
  let (token, device_id) = path.into_inner();
  let auth = authorization_from_token(token.as_str(), &state)?;
  let user_uuid = UserUuid::from_auth(auth)?;
  let result = select_uid_from_uuid(&state.pg_pool, &user_uuid).await;

  match result {
    Ok(uid) => {
      let user_change_recv = state.pg_listeners.subscribe_user_change(uid);
      let realtime_user = Arc::new(RealtimeUserImpl::new(uid, device_id));
      let client = ClientSession::new(
        realtime_user,
        user_change_recv,
        server.get_ref().clone(),
        Duration::from_secs(state.config.websocket.heartbeat_interval as u64),
        Duration::from_secs(state.config.websocket.client_timeout as u64),
      );

      match ws::WsResponseBuilder::new(client, &request, payload)
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
