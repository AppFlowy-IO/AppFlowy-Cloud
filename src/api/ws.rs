use crate::component::auth::LoggedUser;
use crate::state::State;
use actix::Addr;
use actix_web::web::{Data, Path, Payload};
use actix_web::{get, web, HttpRequest, HttpResponse, Result, Scope};
use actix_web_actors::ws;
use realtime::core::{CollabManager, CollabSession};
use secrecy::Secret;
use std::time::Duration;

use realtime::entities::RealtimeUser;

pub fn ws_scope() -> Scope {
  web::scope("/ws").service(establish_ws_connection)
}

#[get("/{token}")]
pub async fn establish_ws_connection(
  request: HttpRequest,
  payload: Payload,
  token: Path<String>,
  state: Data<State>,
  server: Data<Addr<CollabManager>>,
) -> Result<HttpResponse> {
  tracing::trace!("{:?}", request);
  let user = LoggedUser::from_token(&state.config.application.server_key, token.as_str())?;
  let client = CollabSession::new(
    user.into(),
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

impl From<LoggedUser> for RealtimeUser {
  fn from(user: LoggedUser) -> Self {
    Self {
      user_id: Secret::new(user.expose_secret().to_string()),
    }
  }
}
