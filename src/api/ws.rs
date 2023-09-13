use crate::state::State;
use actix::Addr;
use actix_web::web::{Data, Path, Payload};
use actix_web::{get, web, HttpRequest, HttpResponse, Result, Scope};
use actix_web_actors::ws;

use realtime::client::ClientWSSession;
use realtime::collaborate::CollabServer;
use std::time::Duration;

use crate::component::auth::jwt::{authorization_from_token, UserUuid};

use storage::collab::CollabPostgresDBStorageImpl;

pub fn ws_scope() -> Scope {
  web::scope("/ws").service(establish_ws_connection)
}

#[get("/{token}")]
pub async fn establish_ws_connection(
  request: HttpRequest,
  payload: Payload,
  token: Path<String>,
  state: Data<State>,
  server: Data<Addr<CollabServer<CollabPostgresDBStorageImpl>>>,
) -> Result<HttpResponse> {
  tracing::info!("ws connect: {:?}", request);
  let auth = authorization_from_token(token.as_str(), &state)?;
  let user_uuid = UserUuid::from_auth(auth)?;
  let client = ClientWSSession::new(
    user_uuid,
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
