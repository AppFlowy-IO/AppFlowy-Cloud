use std::fmt::{Debug, Display, Formatter};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use yrs::updates::decoder::{Decode, Decoder};
use yrs::updates::encoder::{Encode, Encoder};
use yrs::StateVector;

/// Tag id for [Message::Sync].
pub const MSG_SYNC: u8 = 0;
/// Tag id for [Message::Awareness].
pub const MSG_AWARENESS: u8 = 1;
/// Tag id for [Message::Auth].
pub const MSG_AUTH: u8 = 2;
pub const MSG_CUSTOM: u8 = 3;

pub const PERMISSION_DENIED: u8 = 0;
pub const PERMISSION_GRANTED: u8 = 1;

#[derive(Debug, Eq, PartialEq)]
pub enum Message {
  Sync(SyncMessage),
  Auth(Option<String>),
  Awareness(Vec<u8>),
  Custom(CustomMessage),
}

impl Encode for Message {
  fn encode<E: Encoder>(&self, encoder: &mut E) {
    match self {
      Message::Sync(msg) => {
        encoder.write_var(MSG_SYNC);
        msg.encode(encoder);
      },
      Message::Auth(reason) => {
        encoder.write_var(MSG_AUTH);
        if let Some(reason) = reason {
          encoder.write_var(PERMISSION_DENIED);
          encoder.write_string(reason);
        } else {
          encoder.write_var(PERMISSION_GRANTED);
        }
      },
      Message::Awareness(update) => {
        encoder.write_var(MSG_AWARENESS);
        encoder.write_buf(update)
      },
      Message::Custom(msg) => {
        encoder.write_var(MSG_CUSTOM);
        msg.encode(encoder)
      },
    }
  }
}

impl Decode for Message {
  fn decode<D: Decoder>(decoder: &mut D) -> Result<Self, yrs::encoding::read::Error> {
    let tag: u8 = decoder.read_var()?;
    match tag {
      MSG_SYNC => {
        let msg = SyncMessage::decode(decoder)?;
        Ok(Message::Sync(msg))
      },
      MSG_AWARENESS => {
        let data = decoder.read_buf()?;
        Ok(Message::Awareness(data.into()))
      },
      MSG_AUTH => {
        let reason = if decoder.read_var::<u8>()? == PERMISSION_DENIED {
          Some(decoder.read_string()?.to_string())
        } else {
          None
        };
        Ok(Message::Auth(reason))
      },
      MSG_CUSTOM => {
        let msg = CustomMessage::decode(decoder)?;
        Ok(Message::Custom(msg))
      },
      _ => Err(yrs::encoding::read::Error::UnexpectedValue),
    }
  }
}

impl Display for Message {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      Message::Sync(sync_msg) => f.write_str(&sync_msg.to_string()),
      Message::Auth(_) => f.write_str("Auth"),
      Message::Awareness(_) => f.write_str("Awareness"),
      Message::Custom(msg) => f.write_str(&msg.to_string()),
    }
  }
}

/// Tag id for [CustomMessage::MSG_CUSTOM_START_SYNC].
pub const MSG_CUSTOM_START_SYNC: u8 = 0;

#[derive(Debug, Eq, PartialEq)]
pub enum CustomMessage {
  SyncCheck(SyncMeta),
}

impl Display for CustomMessage {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      CustomMessage::SyncCheck(_) => f.write_str("SyncCheck"),
    }
  }
}
impl Encode for CustomMessage {
  fn encode<E: Encoder>(&self, encoder: &mut E) {
    match self {
      CustomMessage::SyncCheck(msg) => {
        encoder.write_var(MSG_CUSTOM_START_SYNC);
        encoder.write_buf(msg.to_vec());
      },
    }
  }
}

impl Decode for CustomMessage {
  fn decode<D: Decoder>(decoder: &mut D) -> Result<Self, yrs::encoding::read::Error> {
    let tag: u8 = decoder.read_var()?;
    match tag {
      MSG_CUSTOM_START_SYNC => {
        let buf = decoder.read_buf()?;
        let meta = SyncMeta::from_vec(buf)?;
        Ok(CustomMessage::SyncCheck(meta))
      },
      _ => Err(yrs::encoding::read::Error::UnexpectedValue),
    }
  }
}

#[derive(Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct SyncMeta {
  pub(crate) last_sync_at: i64,
}

impl SyncMeta {
  pub fn to_vec(&self) -> Vec<u8> {
    bincode::serialize(self).unwrap()
  }

  pub fn from_vec(data: &[u8]) -> Result<Self, yrs::encoding::read::Error> {
    let meta =
      bincode::deserialize(data).map_err(|_| yrs::encoding::read::Error::UnexpectedValue)?;
    Ok(meta)
  }
}

/// Tag id for [SyncMessage::SyncStep1].
pub const MSG_SYNC_STEP_1: u8 = 0;
/// Tag id for [SyncMessage::SyncStep2].
pub const MSG_SYNC_STEP_2: u8 = 1;
/// Tag id for [SyncMessage::Update].
pub const MSG_SYNC_UPDATE: u8 = 2;

#[derive(Debug, PartialEq, Eq)]
pub enum SyncMessage {
  /// Sync step 1 message contains the [StateVector] from the remote side
  SyncStep1(StateVector),
  /// Sync step 2 message contains the encoded [yrs::Update] from the remote side
  SyncStep2(Vec<u8>),
  /// Update message contains the encoded [yrs::Update] from the remote side
  Update(Vec<u8>),
}

impl Display for SyncMessage {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    match self {
      SyncMessage::SyncStep1(sv) => {
        write!(f, "SyncStep1({:?})", sv)
      },
      SyncMessage::SyncStep2(data) => {
        write!(f, "SyncStep2({})", data.len())
      },
      SyncMessage::Update(data) => {
        write!(f, "Update({})", data.len())
      },
    }
  }
}

impl Encode for SyncMessage {
  fn encode<E: Encoder>(&self, encoder: &mut E) {
    match self {
      SyncMessage::SyncStep1(sv) => {
        encoder.write_var(MSG_SYNC_STEP_1);
        encoder.write_buf(sv.encode_v1());
      },
      SyncMessage::SyncStep2(u) => {
        encoder.write_var(MSG_SYNC_STEP_2);
        encoder.write_buf(u);
      },
      SyncMessage::Update(u) => {
        encoder.write_var(MSG_SYNC_UPDATE);
        encoder.write_buf(u);
      },
    }
  }
}

impl Decode for SyncMessage {
  fn decode<D: Decoder>(decoder: &mut D) -> Result<Self, yrs::encoding::read::Error> {
    let tag: u8 = decoder.read_var()?;
    match tag {
      MSG_SYNC_STEP_1 => {
        let buf = decoder.read_buf()?;
        let sv = StateVector::decode_v1(buf)?;
        Ok(SyncMessage::SyncStep1(sv))
      },
      MSG_SYNC_STEP_2 => {
        let buf = decoder.read_buf()?;
        Ok(SyncMessage::SyncStep2(buf.into()))
      },
      MSG_SYNC_UPDATE => {
        let buf = decoder.read_buf()?;
        Ok(SyncMessage::Update(buf.into()))
      },
      _ => Err(yrs::encoding::read::Error::UnexpectedValue),
    }
  }
}

#[derive(Debug, Error)]
pub enum RTProtocolError {
  /// Incoming Y-protocol message couldn't be deserialized.
  #[error("failed to deserialize message: {0}")]
  DecodingError(#[from] yrs::encoding::read::Error),

  /// Applying incoming Y-protocol awareness update has failed.
  #[error("failed to process awareness update: {0}")]
  YAwareness(#[from] collab::core::awareness::Error),

  /// An incoming Y-protocol authorization request has been denied.
  #[error("permission denied to access: {reason}")]
  PermissionDenied { reason: String },

  /// Thrown whenever an unknown message tag has been sent.
  #[error("unsupported message tag identifier: {0}")]
  Unsupported(u8),

  #[error("{0}")]
  YrsTransaction(String),

  #[error("{0}")]
  YrsApplyUpdate(String),

  #[error("{0}")]
  YrsEncodeState(String),

  #[error(transparent)]
  BinCodeSerde(#[from] bincode::Error),

  #[error("Missing Updates")]
  MissUpdates {
    /// - `state_vector_v1`: Contains the last known state vector from the Collab. If `None`,
    ///   this indicates that the receiver needs to perform a full initialization synchronization starting from sync step 0.
    ///
    /// The receiver uses this information to determine how to recover from the error,
    /// either by recalculating the missing updates based on the `state_vector_v1` if it's available,
    /// or by starting a full initialization sync if it's not.
    state_vector_v1: Option<Vec<u8>>,
    /// - `reason`: A human-readable explanation of why the error was raised, providing context for the missing updates.
    reason: String,
  },

  #[error(transparent)]
  Internal(#[from] anyhow::Error),
}
impl From<std::io::Error> for RTProtocolError {
  fn from(value: std::io::Error) -> Self {
    RTProtocolError::Internal(value.into())
  }
}

/// [MessageReader] can be used over the decoder to read these messages one by one in iterable
/// fashion.
pub struct MessageReader<'a, D: Decoder>(&'a mut D);

impl<'a, D: Decoder> MessageReader<'a, D> {
  pub fn new(decoder: &'a mut D) -> Self {
    MessageReader(decoder)
  }
}

impl<D: Decoder> Iterator for MessageReader<'_, D> {
  type Item = Result<Message, yrs::encoding::read::Error>;

  fn next(&mut self) -> Option<Self::Item> {
    match Message::decode(self.0) {
      Ok(msg) => Some(Ok(msg)),
      Err(yrs::encoding::read::Error::EndOfBuffer(_)) => None,
      Err(error) => Some(Err(error)),
    }
  }
}
