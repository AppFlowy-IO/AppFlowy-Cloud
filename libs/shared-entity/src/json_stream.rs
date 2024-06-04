use crate::response::{AppResponse, AppResponseError};
use app_error::ErrorCode;
use futures::{Stream, TryStreamExt};
use serde::de::DeserializeOwned;
use serde_json::StreamDeserializer;
use std::pin::Pin;
use std::task::{Context, Poll};

impl<T> AppResponse<T>
where
  T: DeserializeOwned + 'static,
{
  pub async fn stream_response(
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
}

#[pin_project::pin_project]
pub struct JsonStream<T> {
  stream: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, AppResponseError>> + Send>>,
  buffer: Vec<u8>,
  _marker: std::marker::PhantomData<T>,
}

impl<T> JsonStream<T> {
  pub fn new<S>(stream: S) -> Self
  where
    S: Stream<Item = Result<bytes::Bytes, AppResponseError>> + Send + 'static,
  {
    JsonStream {
      stream: Box::pin(stream),
      buffer: Vec::new(),
      _marker: std::marker::PhantomData,
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
          let de = StreamDeserializer::new(serde_json::de::SliceRead::new(this.buffer));
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
