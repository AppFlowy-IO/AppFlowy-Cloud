use crate::error::RealtimeError;
use actix::{Message, Recipient};
use app_error::AppError;

use bytes::Bytes;
use collab_entity::CollabType;
use collab_rt_entity::user::RealtimeUser;
pub use collab_rt_entity::RealtimeMessage;
use serde_repr::{Deserialize_repr, Serialize_repr};
use std::fmt::Debug;
use uuid::Uuid;

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Connect {
  pub socket: Recipient<RealtimeMessage>,
  pub user: RealtimeUser,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct Disconnect {
  pub user: RealtimeUser,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct DisconnectByServer;
#[derive(Debug, Clone, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum BusinessID {
  CollabId = 1,
}

#[derive(Debug, Message, Clone)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct ClientWebSocketMessage {
  pub user: RealtimeUser,
  pub message: RealtimeMessage,
}

#[derive(Message)]
#[rtype(result = "Result<(), RealtimeError>")]
pub struct ClientHttpStreamMessage {
  pub uid: i64,
  pub device_id: String,
  pub message: RealtimeMessage,
}

#[derive(Message)]
#[rtype(result = "Result<(), AppError>")]
pub struct ClientHttpUpdateMessage {
  pub user: RealtimeUser,
  pub workspace_id: Uuid,
  pub object_id: Uuid,
  /// Encoded yrs::Update or doc state
  pub update: Bytes,
  /// If the state_vector is not None, it will calculate missing updates base on
  /// given state_vector after apply the update
  pub state_vector: Option<Bytes>,
  pub collab_type: CollabType,
  /// If return_tx is Some, calling await on its receiver will wait until the update was applied
  /// to the collab. The return value will be None if the input state_vector is None.
  pub return_tx: Option<tokio::sync::oneshot::Sender<Result<Option<Vec<u8>>, AppError>>>,
}

#[derive(Message)]
#[rtype(result = "Result<(), AppError>")]
pub struct ClientGenerateEmbeddingMessage {
  pub workspace_id: Uuid,
  pub object_id: Uuid,
  pub return_tx: Option<tokio::sync::oneshot::Sender<Result<(), AppError>>>,
}
