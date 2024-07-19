use std::fmt::{Debug, Display, Formatter};

use bytes::Bytes;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use yrs::encoding::read::Cursor;
use yrs::updates::decoder::DecoderV1;
use yrs::updates::encoder::Encode;

use collab_rt_protocol::{Message, MessageReader};

#[repr(transparent)]
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Payload(Bytes);

impl Serialize for Payload {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    self.0.serialize(serializer)
  }
}

impl<'de> Deserialize<'de> for Payload {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let bytes = Bytes::deserialize(deserializer)?;
    Ok(Payload(bytes))
  }
}

impl Display for Payload {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    write!(f, "{:?}", self.0)
  }
}

#[cfg(feature = "verbose_log")]
impl Debug for Payload {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    let messages = self.messages().unwrap_or_default();
    Debug::fmt(&messages, f)
  }
}

#[cfg(not(feature = "verbose_log"))]
impl Debug for Payload {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    Debug::fmt(&self.0, f)
  }
}

impl Payload {
  pub const EMPTY: Self = Self(Bytes::new());

  pub fn new(message: Message) -> Self {
    Self(message.encode_v1().into())
  }

  pub fn len(&self) -> usize {
    self.0.len()
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  pub fn messages(&self) -> Result<Vec<Message>, yrs::encoding::read::Error> {
    let mut messages = Vec::new();
    for result in self.iter() {
      messages.push(result?);
    }
    Ok(messages)
  }

  pub fn iter(&self) -> PayloadIter {
    PayloadIter::new(&self.0)
  }
}

impl From<Bytes> for Payload {
  fn from(bytes: Bytes) -> Self {
    Self(bytes)
  }
}

impl AsRef<[u8]> for Payload {
  fn as_ref(&self) -> &[u8] {
    &self.0
  }
}

pub struct PayloadIter<'a> {
  reader: MessageReader<'a>,
}

impl<'a> PayloadIter<'a> {
  pub fn new(bytes: &'a Bytes) -> Self {
    let decoder = DecoderV1::new(Cursor::new(bytes));
    let reader = MessageReader::new(decoder);
    Self { reader }
  }
}

impl<'a> Iterator for PayloadIter<'a> {
  type Item = Result<Message, yrs::encoding::read::Error>;

  fn next(&mut self) -> Option<Self::Item> {
    self.reader.next()
  }
}
