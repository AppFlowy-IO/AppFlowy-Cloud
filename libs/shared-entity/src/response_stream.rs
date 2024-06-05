use crate::response::{AppResponse, AppResponseError};
use app_error::ErrorCode;
use bytes::{Bytes, BytesMut};
use database_entity::dto::ChatMessage;
use futures::{Stream, TryStreamExt};
use pin_project::pin_project;
use serde::de::DeserializeOwned;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

impl<T> AppResponse<T>
where
  T: DeserializeOwned + 'static,
{
  pub async fn json_response_stream(
    resp: reqwest::Response,
  ) -> Result<impl Stream<Item = Result<T, AppResponseError>>, AppResponseError> {
    let status_code = resp.status();
    if !status_code.is_success() {
      let body = resp.text().await?;
      return Err(AppResponseError::new(ErrorCode::Internal, body));
    }

    let stream = resp.bytes_stream().map_err(AppResponseError::from);
    Ok(JsonStream::new(stream))
  }

  pub async fn new_line_response_stream(
    resp: reqwest::Response,
  ) -> Result<impl Stream<Item = Result<String, AppResponseError>>, AppResponseError> {
    let status_code = resp.status();
    if !status_code.is_success() {
      let body = resp.text().await?;
      return Err(AppResponseError::new(ErrorCode::Internal, body));
    }

    let stream = resp.bytes_stream().map_err(AppResponseError::from);
    Ok(NewlineStream::new(stream))
  }
  pub async fn answer_response_stream(
    resp: reqwest::Response,
  ) -> Result<
    impl Stream<Item = Result<EitherStringOrMessage<String, ChatMessage>, AppResponseError>>,
    AppResponseError,
  > {
    let status_code = resp.status();
    if !status_code.is_success() {
      let body = resp.text().await?;
      return Err(AppResponseError::new(ErrorCode::Internal, body));
    }

    let stream = resp.bytes_stream().map_err(AppResponseError::from);
    Ok(AnswerStream::new(stream))
  }
}

#[pin_project]
pub struct JsonStream<T> {
  stream: Pin<Box<dyn Stream<Item = Result<Bytes, AppResponseError>> + Send>>,
  buffer: Vec<u8>,
  _marker: PhantomData<T>,
}

impl<T> JsonStream<T> {
  pub fn new<S>(stream: S) -> Self
  where
    S: Stream<Item = Result<Bytes, AppResponseError>> + Send + 'static,
  {
    JsonStream {
      stream: Box::pin(stream),
      buffer: Vec::new(),
      _marker: PhantomData,
    }
  }
}

impl<T> Stream for JsonStream<T>
where
  T: DeserializeOwned,
{
  type Item = Result<T, AppResponseError>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let this = self.project();

    loop {
      match futures::ready!(this.stream.as_mut().poll_next(cx)) {
        Some(Ok(bytes)) => {
          this.buffer.extend_from_slice(&bytes);
          let de = serde_json::StreamDeserializer::new(serde_json::de::SliceRead::new(this.buffer));
          let mut iter = de.into_iter();
          if let Some(result) = iter.next() {
            match result {
              Ok(value) => {
                let remaining = iter.byte_offset();
                this.buffer.drain(0..remaining);
                return Poll::Ready(Some(Ok(value)));
              },
              Err(err) => {
                if err.is_eof() {
                  continue;
                } else {
                  return Poll::Ready(Some(Err(AppResponseError::from(err))));
                }
              },
            }
          }
        },
        Some(Err(err)) => return Poll::Ready(Some(Err(err))),
        None => return Poll::Ready(None),
      }
    }
  }
}

/// Represents a stream of text lines delimited by newlines.
#[pin_project]
pub struct NewlineStream {
  #[pin]
  stream: Pin<Box<dyn Stream<Item = Result<Bytes, AppResponseError>> + Send>>,
  buffer: BytesMut,
}

impl NewlineStream {
  pub fn new<S>(stream: S) -> Self
  where
    S: Stream<Item = Result<Bytes, AppResponseError>> + Send + 'static,
  {
    NewlineStream {
      stream: Box::pin(stream),
      buffer: BytesMut::new(),
    }
  }
}

impl Stream for NewlineStream {
  type Item = Result<String, AppResponseError>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let mut this = self.project();

    loop {
      match futures::ready!(this.stream.as_mut().poll_next(cx)) {
        Some(Ok(bytes)) => {
          this.buffer.extend_from_slice(&bytes);

          if let Some(pos) = this.buffer.iter().position(|&b| b == b'\n') {
            let line = this.buffer.split_to(pos + 1);
            let line = &line[..line.len() - 1]; // Remove the newline character

            match String::from_utf8(line.to_vec()) {
              Ok(value) => return Poll::Ready(Some(Ok(value))),
              Err(err) => return Poll::Ready(Some(Err(AppResponseError::from(err)))),
            }
          }
        },
        Some(Err(err)) => return Poll::Ready(Some(Err(err))),
        None => {
          if !this.buffer.is_empty() {
            match String::from_utf8(this.buffer.to_vec()) {
              Ok(value) => {
                this.buffer.clear();
                return Poll::Ready(Some(Ok(value)));
              },
              Err(err) => return Poll::Ready(Some(Err(AppResponseError::from(err)))),
            }
          } else {
            return Poll::Ready(None);
          }
        },
      }
    }
  }
}

#[pin_project]
pub struct AnswerStream {
  #[pin]
  stream: Pin<Box<dyn Stream<Item = Result<Bytes, AppResponseError>> + Send>>,
  buffer: BytesMut,
  finished: bool,
}

impl AnswerStream {
  pub fn new<S>(stream: S) -> Self
  where
    S: Stream<Item = Result<Bytes, AppResponseError>> + Send + 'static,
  {
    AnswerStream {
      stream: Box::pin(stream),
      buffer: BytesMut::new(),
      finished: false,
    }
  }
}

impl Stream for AnswerStream {
  type Item = Result<EitherStringOrMessage<String, ChatMessage>, AppResponseError>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let mut this = self.project();

    if *this.finished {
      return Poll::Ready(None);
    }

    loop {
      match futures::ready!(this.stream.as_mut().poll_next(cx)) {
        Some(Ok(bytes)) => {
          this.buffer.extend_from_slice(&bytes);

          if let Some(pos) = this.buffer.iter().position(|&b| b == b'\n') {
            let line = this.buffer.split_to(pos + 1);
            let line = &line[..line.len() - 1]; // Remove the newline character

            match String::from_utf8(line.to_vec()) {
              Ok(value) => return Poll::Ready(Some(Ok(EitherStringOrMessage::Left(value)))),
              Err(err) => return Poll::Ready(Some(Err(AppResponseError::from(err)))),
            }
          }
        },
        Some(Err(err)) => return Poll::Ready(Some(Err(err))),
        None => {
          if !this.buffer.is_empty() {
            match String::from_utf8(this.buffer.to_vec()) {
              Ok(value) => {
                this.buffer.clear();
                *this.finished = true;
                match serde_json::from_str::<ChatMessage>(&value) {
                  Ok(message) => {
                    return Poll::Ready(Some(Ok(EitherStringOrMessage::Right(message))));
                  },
                  Err(err) => {
                    return Poll::Ready(Some(Err(AppResponseError::from(err))));
                  },
                }
              },
              Err(err) => return Poll::Ready(Some(Err(AppResponseError::from(err)))),
            }
          } else {
            return Poll::Ready(None);
          }
        },
      }
    }
  }
}

#[derive(Debug)]
pub enum EitherStringOrMessage<L, R> {
  Left(L),
  Right(R),
}
