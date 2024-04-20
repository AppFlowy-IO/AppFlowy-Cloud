use crate::session::RealtimeClient;
use crate::state::AppState;
use actix_web::web::{Data, Payload};
use actix_web::{web, HttpRequest, HttpResponse, Result, Scope};
use actix_web_actors::ws;
use std::collections::HashMap;
use tracing::{debug, error, instrument};

pub fn ws_scope() -> Scope {
  web::scope("/ws").service(web::resource("/v1").route(web::get().to(establish_ws_connection_v1)))
}
const MAX_FRAME_SIZE: usize = 65_536; // 64 KiB

#[instrument(skip_all, err)]
pub async fn establish_ws_connection_v1(
  request: HttpRequest,
  payload: Payload,
  state: Data<AppState>,
  web::Query(_query_params): web::Query<HashMap<String, String>>,
) -> Result<HttpResponse> {
  debug!("🚀new websocket connect: request={:?}", request);

  start_connect(&request, payload, &state).await
}

#[allow(clippy::too_many_arguments)]
#[inline]
async fn start_connect(
  request: &HttpRequest,
  payload: Payload,
  state: &Data<AppState>,
) -> Result<HttpResponse> {
  let session_act = RealtimeClient::new(request.headers().clone().into(), &state.config.ws_client);
  match session_act.connect().await {
    Ok(_) => (),
    Err(e) => {
      error!("🔴ws connection error: {:?}", e);
      return Err(actix_web::error::ErrorInternalServerError(e));
    },
  }
  match ws::WsResponseBuilder::new(session_act, request, payload)
    .frame_size(MAX_FRAME_SIZE * 2)
    .start()
  {
    Ok(response) => Ok(response),
    Err(e) => {
      error!("🔴ws connection error: {:?}", e);
      Err(e)
    },
  }
}
