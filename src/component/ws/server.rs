use crate::component::ws::entities::{Connect, Disconnect, WSError, WebSocketMessage};
use actix::{Actor, Context, Handler};

#[derive(Default)]
pub struct WSServer {}

impl WSServer {
    pub fn new() -> Self {
        WSServer::default()
    }

    pub fn send(&self, _msg: WebSocketMessage) {}
}

impl Actor for WSServer {
    type Context = Context<Self>;
    fn started(&mut self, _ctx: &mut Self::Context) {}
}

impl Handler<Connect> for WSServer {
    type Result = Result<(), WSError>;
    fn handle(&mut self, _msg: Connect, _ctx: &mut Context<Self>) -> Self::Result {
        Ok(())
    }
}

impl Handler<Disconnect> for WSServer {
    type Result = Result<(), WSError>;
    fn handle(&mut self, _msg: Disconnect, _: &mut Context<Self>) -> Self::Result {
        Ok(())
    }
}

impl Handler<WebSocketMessage> for WSServer {
    type Result = ();

    fn handle(&mut self, _msg: WebSocketMessage, _ctx: &mut Context<Self>) -> Self::Result {}
}

impl actix::Supervised for WSServer {
    fn restarting(&mut self, _ctx: &mut Context<WSServer>) {
        tracing::warn!("restarting");
    }
}
