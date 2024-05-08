use futures_util::{Sink, Stream, StreamExt};
use http::HeaderMap;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::{
  tungstenite::{
    error::*,
    protocol::{frame::coding::Data, CloseFrame},
    Message, Result,
  },
  MaybeTlsStream,
};

pub async fn connect_async(url: &str, header_map: HeaderMap) -> crate::Result<WebSocketStream> {
  let mut request = url.into_client_request()?;
  request.headers_mut().extend(header_map);

  let (inner, _response) = tokio_tungstenite::connect_async(request).await?;
  let inner = inner.filter_map(to_fut_message as fn(_) -> _);
  Ok(WebSocketStream { inner })
}

type TokioTungsteniteStream =
  tokio_tungstenite::WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;
type FutMessage = futures_util::future::Ready<Option<crate::Result<crate::Message>>>;
pub struct WebSocketStream {
  inner: futures_util::stream::FilterMap<
    TokioTungsteniteStream,
    FutMessage,
    fn(Result<Message>) -> FutMessage,
  >,
}

impl Stream for WebSocketStream {
  type Item = crate::Result<crate::Message>;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    Pin::new(&mut self.inner).poll_next(cx)
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    self.inner.size_hint()
  }
}

impl Sink<crate::Message> for WebSocketStream {
  type Error = crate::Error;

  fn poll_ready(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
  ) -> Poll<std::result::Result<(), Self::Error>> {
    Pin::new(&mut self.inner).poll_ready(cx).map_err(Into::into)
  }

  fn start_send(
    mut self: Pin<&mut Self>,
    item: crate::Message,
  ) -> std::result::Result<(), Self::Error> {
    Pin::new(&mut self.inner)
      .start_send(item.into())
      .map_err(Into::into)
  }

  fn poll_flush(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
  ) -> Poll<std::result::Result<(), Self::Error>> {
    Pin::new(&mut self.inner).poll_flush(cx).map_err(Into::into)
  }

  fn poll_close(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
  ) -> Poll<std::result::Result<(), Self::Error>> {
    Pin::new(&mut self.inner).poll_close(cx).map_err(Into::into)
  }
}

fn to_fut_message(msg: Result<Message>) -> FutMessage {
  fn inner(msg: Result<Message>) -> Option<crate::Result<crate::Message>> {
    let msg = match msg {
      Ok(msg) => match msg {
        Message::Text(inner) => Ok(crate::Message::Text(inner)),
        Message::Binary(inner) => Ok(crate::Message::Binary(inner)),
        Message::Close(inner) => Ok(crate::Message::Close(inner.map(Into::into))),
        Message::Pong(inner) => Ok(crate::Message::Pong(inner)),
        Message::Ping(inner) => Ok(crate::Message::Ping(inner)),
        Message::Frame(_) => return None,
      },
      Err(err) => Err(crate::Error::from(err)),
    };
    Some(msg)
  }
  futures_util::future::ready(inner(msg))
}

impl<'a> From<CloseFrame<'a>> for crate::message::CloseFrame<'a> {
  fn from(close_frame: CloseFrame<'a>) -> Self {
    crate::message::CloseFrame {
      code: u16::from(close_frame.code).into(),
      reason: close_frame.reason,
    }
  }
}

impl<'a> From<crate::message::CloseFrame<'a>> for CloseFrame<'a> {
  fn from(close_frame: crate::message::CloseFrame<'a>) -> Self {
    CloseFrame {
      code: u16::from(close_frame.code).into(),
      reason: close_frame.reason,
    }
  }
}

impl From<Message> for crate::Message {
  fn from(msg: Message) -> Self {
    match msg {
      Message::Text(inner) => crate::Message::Text(inner),
      Message::Binary(inner) => crate::Message::Binary(inner),
      Message::Close(inner) => crate::Message::Close(inner.map(Into::into)),
      Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {
        unreachable!("Unsendable via interface.")
      },
    }
  }
}

impl From<crate::Message> for Message {
  fn from(msg: crate::Message) -> Self {
    match msg {
      crate::Message::Text(inner) => Message::Text(inner),
      crate::Message::Binary(inner) => Message::Binary(inner),
      crate::Message::Close(inner) => Message::Close(inner.map(Into::into)),
      crate::Message::Ping(data) => Message::Ping(data),
      crate::Message::Pong(data) => Message::Pong(data),
    }
  }
}

impl From<Error> for crate::Error {
  fn from(err: Error) -> Self {
    match err {
      Error::ConnectionClosed => crate::Error::ConnectionClosed,
      Error::AlreadyClosed => crate::Error::AlreadyClosed,
      Error::Io(inner) => crate::Error::Io(inner),
      Error::Tls(inner) => crate::Error::Tls(inner),
      Error::Capacity(inner) => crate::Error::Capacity(inner.into()),
      Error::Protocol(inner) => crate::Error::Protocol(inner.into()),
      Error::WriteBufferFull(inner) => crate::Error::WriteBufferFull(inner.into()),
      Error::Utf8 => crate::Error::Utf8,
      Error::AttackAttempt => crate::Error::AttackAttempt,
      Error::Url(inner) => crate::Error::Url(inner.into()),
      Error::Http(inner) => crate::Error::Http(inner),
      Error::HttpFormat(inner) => crate::Error::HttpFormat(inner),
    }
  }
}

impl From<CapacityError> for crate::error::CapacityError {
  fn from(err: CapacityError) -> Self {
    match err {
      CapacityError::TooManyHeaders => crate::error::CapacityError::TooManyHeaders,
      CapacityError::MessageTooLong { size, max_size } => {
        crate::error::CapacityError::MessageTooLong { size, max_size }
      },
    }
  }
}

impl From<UrlError> for crate::error::UrlError {
  fn from(err: UrlError) -> Self {
    match err {
      UrlError::TlsFeatureNotEnabled => crate::error::UrlError::TlsFeatureNotEnabled,
      UrlError::NoHostName => crate::error::UrlError::NoHostName,
      UrlError::UnableToConnect(inner) => crate::error::UrlError::UnableToConnect(inner),
      UrlError::UnsupportedUrlScheme => crate::error::UrlError::UnsupportedUrlScheme,
      UrlError::EmptyHostName => crate::error::UrlError::EmptyHostName,
      UrlError::NoPathOrQuery => crate::error::UrlError::NoPathOrQuery,
    }
  }
}

impl From<ProtocolError> for crate::error::ProtocolError {
  fn from(err: ProtocolError) -> Self {
    match err {
      ProtocolError::WrongHttpMethod => crate::error::ProtocolError::WrongHttpMethod,
      ProtocolError::WrongHttpVersion => crate::error::ProtocolError::WrongHttpVersion,
      ProtocolError::MissingConnectionUpgradeHeader => {
        crate::error::ProtocolError::MissingConnectionUpgradeHeader
      },
      ProtocolError::MissingUpgradeWebSocketHeader => {
        crate::error::ProtocolError::MissingUpgradeWebSocketHeader
      },
      ProtocolError::MissingSecWebSocketVersionHeader => {
        crate::error::ProtocolError::MissingSecWebSocketVersionHeader
      },
      ProtocolError::MissingSecWebSocketKey => crate::error::ProtocolError::MissingSecWebSocketKey,
      ProtocolError::SecWebSocketAcceptKeyMismatch => {
        crate::error::ProtocolError::SecWebSocketAcceptKeyMismatch
      },
      ProtocolError::JunkAfterRequest => crate::error::ProtocolError::JunkAfterRequest,
      ProtocolError::CustomResponseSuccessful => {
        crate::error::ProtocolError::CustomResponseSuccessful
      },
      ProtocolError::InvalidHeader(header_name) => {
        crate::error::ProtocolError::InvalidHeader(header_name)
      },
      ProtocolError::HandshakeIncomplete => crate::error::ProtocolError::HandshakeIncomplete,
      ProtocolError::HttparseError(inner) => crate::error::ProtocolError::HttparseError(inner),
      ProtocolError::SendAfterClosing => crate::error::ProtocolError::SendAfterClosing,
      ProtocolError::ReceivedAfterClosing => crate::error::ProtocolError::ReceivedAfterClosing,
      ProtocolError::NonZeroReservedBits => crate::error::ProtocolError::NonZeroReservedBits,
      ProtocolError::UnmaskedFrameFromClient => {
        crate::error::ProtocolError::UnmaskedFrameFromClient
      },
      ProtocolError::MaskedFrameFromServer => crate::error::ProtocolError::MaskedFrameFromServer,
      ProtocolError::FragmentedControlFrame => crate::error::ProtocolError::FragmentedControlFrame,
      ProtocolError::ControlFrameTooBig => crate::error::ProtocolError::ControlFrameTooBig,
      ProtocolError::UnknownControlFrameType(inner) => {
        crate::error::ProtocolError::UnknownControlFrameType(inner)
      },
      ProtocolError::UnknownDataFrameType(inner) => {
        crate::error::ProtocolError::UnknownDataFrameType(inner)
      },
      ProtocolError::UnexpectedContinueFrame => {
        crate::error::ProtocolError::UnexpectedContinueFrame
      },
      ProtocolError::ExpectedFragment(inner) => {
        crate::error::ProtocolError::ExpectedFragment(inner.into())
      },
      ProtocolError::ResetWithoutClosingHandshake => {
        crate::error::ProtocolError::ResetWithoutClosingHandshake
      },
      ProtocolError::InvalidOpcode(inner) => crate::error::ProtocolError::InvalidOpcode(inner),
      ProtocolError::InvalidCloseSequence => crate::error::ProtocolError::InvalidCloseSequence,
    }
  }
}

impl From<Data> for crate::error::Data {
  fn from(data: Data) -> Self {
    match data {
      Data::Continue => crate::error::Data::Continue,
      Data::Text => crate::error::Data::Text,
      Data::Binary => crate::error::Data::Binary,
      Data::Reserved(inner) => crate::error::Data::Reserved(inner),
    }
  }
}
