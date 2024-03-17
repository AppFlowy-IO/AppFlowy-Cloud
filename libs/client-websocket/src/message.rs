/// An enum representing the various forms of a WebSocket message.
#[derive(Debug, Eq, PartialEq, Clone)]
pub enum Message {
  /// A text WebSocket message
  Text(String),
  /// A binary WebSocket message
  Binary(Vec<u8>),
  /// A close message with the optional close frame.
  Close(Option<CloseFrame<'static>>),
  Ping(Vec<u8>),
  Pong(Vec<u8>),
}

impl Message {
  /// Create a new text WebSocket message from a stringable.
  pub fn text<S>(string: S) -> Message
  where
    S: Into<String>,
  {
    Message::Text(string.into())
  }

  /// Create a new binary WebSocket message by converting to Vec<u8>.
  pub fn binary<B>(bin: B) -> Message
  where
    B: Into<Vec<u8>>,
  {
    Message::Binary(bin.into())
  }

  /// Indicates whether a message is a text message.
  pub fn is_text(&self) -> bool {
    matches!(*self, Message::Text(_))
  }

  /// Indicates whether a message is a binary message.
  pub fn is_binary(&self) -> bool {
    matches!(*self, Message::Binary(_))
  }

  /// Indicates whether a message is a ping message.
  pub fn is_ping(&self) -> bool {
    matches!(self, Message::Ping(_))
  }

  /// Indicates whether a message is a pong message.
  pub fn is_pong(&self) -> bool {
    matches!(self, Message::Pong(_))
  }

  /// Indicates whether a message ia s close message.
  pub fn is_close(&self) -> bool {
    matches!(*self, Message::Close(_))
  }

  /// Get the length of the WebSocket message.
  pub fn len(&self) -> usize {
    match self {
      Message::Text(s) => s.len(),
      Message::Binary(data) => data.len(),
      Message::Close(data) => data.as_ref().map(|d| d.reason.len()).unwrap_or(0),
      Message::Ping(data) => data.len(),
      Message::Pong(data) => data.len(),
    }
  }

  /// Returns true if the WebSocket message has no content.
  /// For example, if the other side of the connection sent an empty string.
  pub fn is_empty(&self) -> bool {
    self.len() == 0
  }

  /// Consume the WebSocket and return it as binary data.
  pub fn into_data(self) -> Vec<u8> {
    match self {
      Message::Text(string) => string.into_bytes(),
      Message::Binary(data) => data,
      Message::Close(None) => Vec::new(),
      Message::Close(Some(frame)) => frame.reason.into_owned().into_bytes(),
      Message::Ping(data) => data,
      Message::Pong(data) => data,
    }
  }

  /// Attempt to consume the WebSocket message and convert it to a String.
  pub fn into_text(self) -> Result<String, crate::Error> {
    match self {
      Message::Text(string) => Ok(string),
      Message::Binary(data) => Ok(String::from_utf8(data).map_err(|err| err.utf8_error())?),
      Message::Close(None) => Ok(String::new()),
      Message::Close(Some(frame)) => Ok(frame.reason.into_owned()),
      Message::Ping(data) => Ok(String::from_utf8(data).map_err(|err| err.utf8_error())?),
      Message::Pong(data) => Ok(String::from_utf8(data).map_err(|err| err.utf8_error())?),
    }
  }

  /// Attempt to get a &str from the WebSocket message,
  /// this will try to convert binary data to utf8.
  pub fn to_text(&self) -> Result<&str, crate::Error> {
    match self {
      Message::Text(s) => Ok(s.as_str()),
      Message::Binary(data) => Ok(std::str::from_utf8(data)?),
      Message::Close(None) => Ok(""),
      Message::Close(Some(ref frame)) => Ok(&frame.reason),
      Message::Ping(data) => Ok(std::str::from_utf8(data)?),
      Message::Pong(data) => Ok(std::str::from_utf8(data)?),
    }
  }
}

impl From<String> for Message {
  fn from(string: String) -> Self {
    Message::text(string)
  }
}

impl<'s> From<&'s str> for Message {
  fn from(string: &'s str) -> Self {
    Message::text(string)
  }
}

impl<'b> From<&'b [u8]> for Message {
  fn from(data: &'b [u8]) -> Self {
    Message::binary(data)
  }
}

impl From<Vec<u8>> for Message {
  fn from(data: Vec<u8>) -> Self {
    Message::binary(data)
  }
}

impl From<Message> for Vec<u8> {
  fn from(message: Message) -> Self {
    message.into_data()
  }
}

impl std::convert::TryFrom<Message> for String {
  type Error = crate::Error;

  fn try_from(value: Message) -> std::result::Result<Self, Self::Error> {
    value.into_text()
  }
}

impl std::fmt::Display for Message {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::result::Result<(), std::fmt::Error> {
    if let Ok(string) = self.to_text() {
      write!(f, "{}", string)
    } else {
      write!(f, "Binary Data<length={}>", self.len())
    }
  }
}

/// A struct representing the close command.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct CloseFrame<'t> {
  /// The reason as a code.
  pub code: coding::CloseCode,
  /// The reason as text string.
  pub reason: std::borrow::Cow<'t, str>,
}

impl<'t> CloseFrame<'t> {
  /// Convert into a owned string.
  pub fn into_owned(self) -> CloseFrame<'static> {
    CloseFrame {
      code: self.code,
      reason: self.reason.into_owned().into(),
    }
  }
}

impl<'t> std::fmt::Display for CloseFrame<'t> {
  fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
    write!(f, "{} ({})", self.reason, self.code)
  }
}

pub mod coding {
  use self::CloseCode::*;

  /// Status code used to indicate why an endpoint is closing the WebSocket connection.
  #[derive(Debug, Eq, PartialEq, Clone, Copy)]
  pub enum CloseCode {
    /// Indicates a normal closure, meaning that the purpose for
    /// which the connection was established has been fulfilled.
    Normal,
    /// Indicates that an endpoint is "going away", such as a server
    /// going down or a browser having navigated away from a page.
    Away,
    /// Indicates that an endpoint is terminating the connection due
    /// to a protocol error.
    Protocol,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a type of data it cannot accept (e.g., an
    /// endpoint that understands only text data MAY send this if it
    /// receives a binary message).
    Unsupported,
    /// Indicates that no status code was included in a closing frame. This
    /// close code makes it possible to use a single method, `on_close` to
    /// handle even cases where no close code was provided.
    Status,
    /// Indicates an abnormal closure. If the abnormal closure was due to an
    /// error, this close code will not be used. Instead, the `on_error` method
    /// of the handler will be called with the error. However, if the connection
    /// is simply dropped, without an error, this close code will be sent to the
    /// handler.
    Abnormal,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received data within a message that was not
    /// consistent with the type of the message (e.g., non-UTF-8 \[RFC3629\]
    /// data within a text message).
    Invalid,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that violates its policy.  This
    /// is a generic status code that can be returned when there is no
    /// other more suitable status code (e.g., Unsupported or Size) or if there
    /// is a need to hide specific details about the policy.
    Policy,
    /// Indicates that an endpoint is terminating the connection
    /// because it has received a message that is too big for it to
    /// process.
    Size,
    /// Indicates that an endpoint (client) is terminating the
    /// connection because it has expected the server to negotiate one or
    /// more extension, but the server didn't return them in the response
    /// message of the WebSocket handshake.  The list of extensions that
    /// are needed should be given as the reason for closing.
    /// Note that this status code is not used by the server, because it
    /// can fail the WebSocket handshake instead.
    Extension,
    /// Indicates that a server is terminating the connection because
    /// it encountered an unexpected condition that prevented it from
    /// fulfilling the request.
    Error,
    /// Indicates that the server is restarting. A client may choose to reconnect,
    /// and if it does, it should use a randomized delay of 5-30 seconds between attempts.
    Restart,
    /// Indicates that the server is overloaded and the client should either connect
    /// to a different IP (when multiple targets exist), or reconnect to the same IP
    /// when a user has performed an action.
    Again,
    #[doc(hidden)]
    Tls,
    #[doc(hidden)]
    Reserved(u16),
    #[doc(hidden)]
    Iana(u16),
    #[doc(hidden)]
    Library(u16),
    #[doc(hidden)]
    Bad(u16),
  }

  impl CloseCode {
    /// Check if this CloseCode is allowed.
    pub fn is_allowed(self) -> bool {
      !matches!(self, Bad(_) | Reserved(_) | Status | Abnormal | Tls)
    }
  }

  impl std::fmt::Display for CloseCode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
      let code: u16 = self.into();
      write!(f, "{}", code)
    }
  }

  impl From<CloseCode> for u16 {
    fn from(code: CloseCode) -> u16 {
      match code {
        Normal => 1000,
        Away => 1001,
        Protocol => 1002,
        Unsupported => 1003,
        Status => 1005,
        Abnormal => 1006,
        Invalid => 1007,
        Policy => 1008,
        Size => 1009,
        Extension => 1010,
        Error => 1011,
        Restart => 1012,
        Again => 1013,
        Tls => 1015,
        Reserved(code) => code,
        Iana(code) => code,
        Library(code) => code,
        Bad(code) => code,
      }
    }
  }

  impl<'t> From<&'t CloseCode> for u16 {
    fn from(code: &'t CloseCode) -> u16 {
      (*code).into()
    }
  }

  impl From<u16> for CloseCode {
    fn from(code: u16) -> CloseCode {
      match code {
        1000 => Normal,
        1001 => Away,
        1002 => Protocol,
        1003 => Unsupported,
        1005 => Status,
        1006 => Abnormal,
        1007 => Invalid,
        1008 => Policy,
        1009 => Size,
        1010 => Extension,
        1011 => Error,
        1012 => Restart,
        1013 => Again,
        1015 => Tls,
        1..=999 => Bad(code),
        1016..=2999 => Reserved(code),
        3000..=3999 => Iana(code),
        4000..=4999 => Library(code),
        _ => Bad(code),
      }
    }
  }
}
