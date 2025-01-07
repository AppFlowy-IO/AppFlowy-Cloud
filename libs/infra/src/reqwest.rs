use anyhow::anyhow;
use anyhow::Error;
use bytes::{Bytes, BytesMut};
use futures::{ready, Stream};
use std::marker::PhantomData;
use std::pin::Pin;

use pin_project::pin_project;
use serde::de::DeserializeOwned;
use serde_json::de::SliceRead;
use serde_json::StreamDeserializer;
use std::error::Error as StdError;
use std::task::{Context, Poll};

pub async fn check_response(resp: reqwest::Response) -> Result<(), Error> {
  let status_code = resp.status();
  if !status_code.is_success() {
    let body = resp.text().await?;
    anyhow::bail!("got error code: {}, body: {}", status_code, body)
  }
  resp.bytes().await?;
  Ok(())
}

pub async fn from_response<T>(resp: reqwest::Response) -> Result<T, Error>
where
  T: serde::de::DeserializeOwned,
{
  let status_code = resp.status();
  if !status_code.is_success() {
    let body = resp.text().await?;
    anyhow::bail!("got error code: {}, body: {}", status_code, body)
  }
  from_body(resp).await
}

pub async fn from_body<T>(resp: reqwest::Response) -> Result<T, Error>
where
  T: serde::de::DeserializeOwned,
{
  let status_code = resp.status();
  let bytes = resp.bytes().await?;
  serde_json::from_slice(&bytes).map_err(|e| {
    anyhow!(
      "deserialize error: {}, status: {}, body: {}",
      status_code,
      e,
      String::from_utf8_lossy(&bytes)
    )
  })
}

#[pin_project]
pub struct JsonStream<T, E, SE> {
  #[pin]
  stream: Pin<Box<dyn Stream<Item = Result<Bytes, E>> + Send>>,
  buffer: Vec<u8>,
  _marker: PhantomData<T>,
  _marker_error: PhantomData<SE>,
}

impl<T, E, SE> JsonStream<T, E, SE>
where
  E: From<SE> + From<serde_json::Error> + std::error::Error + Send + Sync + 'static,
  SE: std::error::Error + Send + Sync + 'static,
{
  pub fn new<S>(stream: S) -> Self
  where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
  {
    JsonStream {
      stream: Box::pin(stream),
      buffer: Vec::new(),
      _marker: PhantomData,
      _marker_error: PhantomData,
    }
  }
}

impl<T, E, SE> Stream for JsonStream<T, E, SE>
where
  T: DeserializeOwned,
  E: From<SE> + From<serde_json::Error> + std::error::Error + Send + Sync + 'static,
  SE: std::error::Error + Send + Sync + 'static,
{
  type Item = Result<T, E>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let mut this = self.project();

    loop {
      // Poll for the next chunk of data from the underlying stream
      match ready!(this.stream.as_mut().poll_next(cx)) {
        Some(Ok(bytes)) => {
          this.buffer.extend_from_slice(&bytes);

          // Create a StreamDeserializer to deserialize the bytes into T
          let mut de = StreamDeserializer::new(SliceRead::new(this.buffer));

          // Check if there's a valid deserialized object in the stream
          match de.next() {
            Some(Ok(value)) => {
              // Determine the offset of the successfully deserialized data
              let offset = de.byte_offset();
              // Drain the buffer up to the byte offset to remove the consumed bytes
              this.buffer.drain(0..offset);
              return Poll::Ready(Some(Ok(value)));
            },
            Some(Err(err)) if err.is_eof() => {
              // Wait for more data if EOF indicates incomplete data
              return Poll::Pending;
            },
            Some(Err(err)) => {
              return Poll::Ready(Some(Err(err.into())));
            },
            None => {
              // No complete object is ready, wait for more data
              continue;
            },
          }
        },
        Some(Err(err)) => {
          // Convert the error to SE
          return Poll::Ready(Some(Err(err)));
        },
        None => {
          // Stream has ended; handle any remaining data in the buffer
          if this.buffer.is_empty() {
            return Poll::Ready(None);
          }

          // Try to deserialize any remaining data in the buffer
          let mut de = StreamDeserializer::new(SliceRead::new(this.buffer));
          match de.next() {
            Some(Ok(value)) => {
              let offset = de.byte_offset();
              this.buffer.drain(0..offset);
              return Poll::Ready(Some(Ok(value)));
            },
            Some(Err(err)) if err.is_eof() => {
              // If EOF and buffer is incomplete, return None to indicate stream end
              return Poll::Ready(None);
            },
            Some(Err(err)) => {
              // Return any other errors that occur during deserialization
              return Poll::Ready(Some(Err(err.into())));
            },
            None => {
              // No more data to process; end the stream
              return Poll::Ready(None);
            },
          }
        },
      }
    }
  }
}

/// Represents a stream of text lines delimited by newlines.
#[pin_project]
pub struct NewlineStream<E> {
  #[pin]
  stream: Pin<Box<dyn Stream<Item = Result<Bytes, E>> + Send>>,
  buffer: BytesMut,
  _marker: PhantomData<E>,
}

impl<E> NewlineStream<E> {
  pub fn new<S>(stream: S) -> Self
  where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
  {
    NewlineStream {
      stream: Box::pin(stream),
      buffer: BytesMut::new(),
      _marker: PhantomData,
    }
  }
}

impl<E> Stream for NewlineStream<E>
where
  E: StdError + Send + Sync + 'static + From<std::string::FromUtf8Error>,
{
  type Item = Result<String, E>;

  fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
    let mut this = self.project();

    loop {
      match ready!(this.stream.as_mut().poll_next(cx)) {
        Some(Ok(bytes)) => {
          this.buffer.extend_from_slice(&bytes);
          if let Some(pos) = this.buffer.iter().position(|&b| b == b'\n') {
            let line = this.buffer.split_to(pos + 1);
            let line = &line[..line.len() - 1]; // Remove the newline character

            match String::from_utf8(line.to_vec()) {
              Ok(value) => return Poll::Ready(Some(Ok(value))),
              Err(err) => return Poll::Ready(Some(Err(E::from(err)))),
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
              Err(err) => return Poll::Ready(Some(Err(E::from(err)))),
            }
          } else {
            return Poll::Ready(None);
          }
        },
      }
    }
  }
}
