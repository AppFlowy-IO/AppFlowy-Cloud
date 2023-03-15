use crate::component::auth::LoggedUser;
use crate::component::ws::{MessageReceivers, WSClient, WSServer};
use crate::state::State;
use actix::Addr;
use actix_web::web::{Data, Path, Payload};
use actix_web::{get, web, HttpRequest, HttpResponse, Result, Scope};
use actix_web_actors::ws;

pub fn ws_scope() -> Scope {
    web::scope("/ws").service(establish_ws_connection)
}

#[get("/{token}")]
pub async fn establish_ws_connection(
    request: HttpRequest,
    payload: Payload,
    token: Path<String>,
    state: Data<State>,
    server: Data<Addr<WSServer>>,
    msg_receivers: Data<MessageReceivers>,
) -> Result<HttpResponse> {
    tracing::info!("establish_ws_connection");
    let user = LoggedUser::from_token(&state.config.application.server_key, token.as_str())?;
    let client = WSClient::new(user, server.get_ref().clone(), msg_receivers);
    match ws::start(client, &request, payload) {
        Ok(response) => Ok(response),
        Err(e) => {
            tracing::error!("ws connection error: {:?}", e);
            Err(e)
        }
    }
}
