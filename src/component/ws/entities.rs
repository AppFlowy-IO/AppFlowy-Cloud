use crate::component::auth::LoggedUser;
use actix::{Message, Recipient};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::fmt::Formatter;
use std::sync::Arc;

pub type Socket = Recipient<WebSocketMessage>;

#[derive(Serialize, Deserialize, Debug, Clone, Hash, PartialEq, Eq)]
pub struct WSSessionId(pub String);

impl<T: AsRef<str>> std::convert::From<T> for WSSessionId {
    fn from(s: T) -> Self {
        WSSessionId(s.as_ref().to_owned())
    }
}

impl std::fmt::Display for WSSessionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let desc = &self.0.to_string();
        f.write_str(desc)
    }
}

pub struct Session {
    pub user: Arc<LoggedUser>,
    pub socket: Socket,
}

impl std::convert::From<Connect> for Session {
    fn from(c: Connect) -> Self {
        Self {
            user: c.user,
            socket: c.socket,
        }
    }
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), WSError>")]
pub struct Connect {
    pub socket: Socket,
    pub user: Arc<LoggedUser>,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), WSError>")]
pub struct Disconnect {
    pub user: Arc<LoggedUser>,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "()")]
pub struct WebSocketMessage(pub Bytes);

impl std::ops::Deref for WebSocketMessage {
    type Target = Bytes;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SocketMessagePayload {
    pub(crate) channel: u8,
    pub(crate) data: Vec<u8>,
}

impl SocketMessagePayload {
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn from_bytes<T: AsRef<[u8]>>(bytes: T) -> Self {
        bincode::deserialize(bytes.as_ref()).unwrap()
    }
}

#[derive(Debug)]
pub enum WSError {
    Internal,
}
