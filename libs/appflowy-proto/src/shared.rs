use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use uuid::Uuid;

pub type WorkspaceId = Uuid;
pub type ObjectId = Uuid;

/// Redis stream message ID, parsed.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Default, Hash)]
pub struct Rid {
  pub timestamp: u64,
  pub seq_no: u16,
}

impl Rid {
  pub fn new(timestamp: u64, seq_no: u16) -> Self {
    Rid { timestamp, seq_no }
  }

  pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
    if bytes.len() != 10 {
      return Err(Error::InvalidRid);
    }
    let timestamp = u64::from_be_bytes([
      bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ]);
    let seq_no = u16::from_be_bytes([bytes[8], bytes[9]]);
    Ok(Rid { timestamp, seq_no })
  }

  pub fn into_bytes(&self) -> [u8; 10] {
    let mut bytes = [0; 10];
    bytes[0..8].copy_from_slice(&self.timestamp.to_be_bytes());
    bytes[8..10].copy_from_slice(&self.seq_no.to_be_bytes());
    bytes
  }
}

impl Display for Rid {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}-{}", self.timestamp, self.seq_no)
  }
}

impl FromStr for Rid {
  type Err = String;

  fn from_str(s: &str) -> Result<Self, Self::Err> {
    let mut parts = s.split('-');
    let timestamp = parts
      .next()
      .ok_or("missing timestamp")?
      .parse()
      .map_err(|e| format!("{}", e))?;
    let seq_no = parts
      .next()
      .ok_or("missing sequence number")?
      .parse()
      .map_err(|e| format!("{}", e))?;
    Ok(Rid { timestamp, seq_no })
  }
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, Default, Eq, PartialEq)]
pub enum UpdateFlags {
  #[default]
  Lib0v1 = 0,
  Lib0v2 = 1,
}

impl TryFrom<u8> for UpdateFlags {
  type Error = Error;

  fn try_from(value: u8) -> Result<Self, Self::Error> {
    match value {
      0 => Ok(UpdateFlags::Lib0v1),
      1 => Ok(UpdateFlags::Lib0v2),
      tag => Err(Error::UnsupportedFlag(tag)),
    }
  }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
  #[error("failed to decode message: {0}")]
  ProtobufDecode(#[from] prost::DecodeError),
  #[error("failed to encode message: {0}")]
  ProtobufEncode(#[from] prost::EncodeError),
  #[error("failed to decode object id: {0}")]
  InvalidObjectId(#[from] uuid::Error),
  #[error("failed to decode Redis stream message ID")]
  InvalidRid,
  #[error("failed to decode message: missing fields")]
  MissingFields,
  #[error("failed to decode message: unsupported flag for update: {0}")]
  UnsupportedFlag(u8),
  #[error("failed to decode message: unknown collab type: {0}")]
  UnknownCollabType(u8),
  #[error("Message does not match expected client message")]
  UnsupportedClientMessage,
}
