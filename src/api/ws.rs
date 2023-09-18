use crate::state::AppState;
use actix::Addr;
use actix_web::web::{Data, Path, Payload};
use actix_web::{get, web, HttpRequest, HttpResponse, Result, Scope};
use actix_web_actors::ws;
use std::sync::Arc;

use realtime::client::{ClientWSSession, RealtimeUserImpl};
use realtime::collaborate::CollabServer;

use std::time::Duration;

use crate::component::auth::jwt::{authorization_from_token, UserUuid};

use crate::component::storage_proxy::CollabStorageProxy;

pub fn ws_scope() -> Scope {
  web::scope("/ws").service(establish_ws_connection)
}

#[get("/{token}/{device_id}")]
pub async fn establish_ws_connection(
  request: HttpRequest,
  payload: Payload,
  path: Path<(String, String)>,
  state: Data<AppState>,
  server: Data<Addr<CollabServer<CollabStorageProxy, Arc<RealtimeUserImpl>>>>,
) -> Result<HttpResponse> {
  tracing::info!("ws connect: {:?}", request);
  let (token, device_id) = path.into_inner();
  let auth = authorization_from_token(token.as_str(), &state)?;
  let user_uuid = UserUuid::from_auth(auth)?;
  let realtime_user = Arc::new(RealtimeUserImpl::new(user_uuid.to_string(), device_id));
  let client = ClientWSSession::new(
    realtime_user,
    server.get_ref().clone(),
    Duration::from_secs(state.config.websocket.heartbeat_interval as u64),
    Duration::from_secs(state.config.websocket.client_timeout as u64),
  );

  match ws::start(client, &request, payload) {
    Ok(response) => Ok(response),
    Err(e) => {
      tracing::error!("🔴ws connection error: {:?}", e);
      Err(e)
    },
  }
}
