use crate::component::ws::entities::{
    Connect, Disconnect, Session, WSError, WSSessionId, WebSocketMessage,
};
use actix::{Actor, Context, Handler};
use dashmap::DashMap;

pub struct WSServer {
    sessions: DashMap<WSSessionId, Session>,
}

impl std::default::Default for WSServer {
    fn default() -> Self {
        Self {
            sessions: DashMap::new(),
        }
    }
}
impl WSServer {
    pub fn new() -> Self {
        WSServer::default()
    }

    pub fn send(&self, _msg: WebSocketMessage) {
        unimplemented!()
    }
}

impl Actor for WSServer {
    type Context = Context<Self>;
    fn started(&mut self, _ctx: &mut Self::Context) {}
}

impl Handler<Connect> for WSServer {
    type Result = Result<(), WSError>;
    fn handle(&mut self, msg: Connect, _ctx: &mut Context<Self>) -> Self::Result {
        let session: Session = msg.into();
        self.sessions.insert(session.id.clone(), session);
        Ok(())
    }
}

impl Handler<Disconnect> for WSServer {
    type Result = Result<(), WSError>;
    fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) -> Self::Result {
        self.sessions.remove(&msg.sid);
        Ok(())
    }
}

impl Handler<WebSocketMessage> for WSServer {
    type Result = ();

    fn handle(&mut self, _msg: WebSocketMessage, _ctx: &mut Context<Self>) -> Self::Result {
        unimplemented!()
    }
}

impl actix::Supervised for WSServer {
    fn restarting(&mut self, _ctx: &mut Context<WSServer>) {
        tracing::warn!("restarting");
    }
}
