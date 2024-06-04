use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Cow;

use app_error::AppError;
pub use app_error::ErrorCode;
use futures::stream::Stream;
use futures::TryStreamExt;
use serde::de::DeserializeOwned;
use serde_json::StreamDeserializer;
use std::fmt::{Debug, Display};
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(feature = "cloud")]
pub use crate::response_actix::*;

/// A macro to generate static AppResponse functions with predefined error codes.
macro_rules! static_app_response {
  ($name:ident, $error:expr) => {
    #[allow(non_snake_case, missing_docs)]
    pub fn $name() -> AppResponse<T> {
      AppResponse::new($error.code(), $error.to_string())
    }
  }; // ($name:ident, $code:expr, $msg:expr) => {
     //   #[allow(non_snake_case, missing_docs)]
     //   pub fn $name(msg: $msg) -> AppResponse<T> {
     //     AppResponse::new($code, $code(msg.to_string()).to_string())
     //   }
     // };
}

/// Represents a standardized application response.
///
/// This structure is used to send consistent responses from the server,
/// containing optional data, an error code, and a message.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppResponse<T> {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub data: Option<T>,

  #[serde(deserialize_with = "default_error_code")]
  pub code: ErrorCode,

  #[serde(default)]
  pub message: Cow<'static, str>,
}

impl<T> AppResponse<T> {
  pub fn new<M: Into<Cow<'static, str>>>(code: ErrorCode, message: M) -> Self {
    Self {
      data: None,
      code,
      message: message.into(),
    }
  }

  static_app_response!(Ok, AppError::Ok);

  pub fn split(self) -> (Option<T>, AppResponseError) {
    if self.is_ok() {
      (self.data, AppResponseError::new(self.code, self.message))
    } else {
      (None, AppResponseError::new(self.code, self.message))
    }
  }

  pub fn into_data(self) -> Result<T, AppResponseError> {
    if self.is_ok() {
      match self.data {
        None => Err(AppResponseError::from(AppError::MissingPayload(
          "".to_string(),
        ))),
        Some(data) => Ok(data),
      }
    } else {
      Err(AppResponseError::new(self.code, self.message))
    }
  }

  pub fn into_error(self) -> Result<(), AppResponseError> {
    if matches!(self.code, ErrorCode::Ok) {
      Ok(())
    } else {
      Err(AppResponseError::new(self.code, self.message))
    }
  }

  pub fn with_data(mut self, data: T) -> Self {
    self.data = Some(data);
    self
  }

  pub fn with_message(mut self, message: impl Into<Cow<'static, str>>) -> Self {
    self.message = message.into();
    self
  }

  pub fn with_code(mut self, code: ErrorCode) -> Self {
    self.code = code;
    self
  }

  pub fn is_ok(&self) -> bool {
    matches!(self.code, ErrorCode::Ok)
  }
}

impl<T> Display for AppResponse<T>
where
  T: Display,
{
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!("{:?}:{}", self.code, self.message))
  }
}

impl<T> std::error::Error for AppResponse<T> where T: Debug + Display {}

/// Provides a conversion from `T1` to `AppResponse<T>`.
///
/// This implementation allows for automatic conversion of any type `T1` that can be
/// transformed into an `AppError` into an `AppResponse<T>`. This is useful for
/// seamlessly converting various error types into a standardized application response.
///
impl<T, T1> From<T1> for AppResponse<T>
where
  T1: Into<AppResponseError>,
{
  fn from(value: T1) -> Self {
    let err: AppResponseError = value.into();
    AppResponse::new(err.code, err.message)
  }
}

impl<T> AppResponse<T>
where
  T: serde::de::DeserializeOwned + 'static,
{
  pub async fn from_response(resp: reqwest::Response) -> Result<Self, anyhow::Error> {
    let status_code = resp.status();
    if !status_code.is_success() {
      let body = resp.text().await?;
      anyhow::bail!("got error code: {}, body: {}", status_code, body)
    }

    let bytes = resp.bytes().await?;
    let resp = serde_json::from_slice(&bytes)?;
    Ok(resp)
  }

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

    // let stream = resp.bytes_stream().map(|item| {
    //   item.map_err(AppResponseError::from).and_then(|bytes| {
    //     match serde_json::from_slice::<T>(bytes.as_ref()) {
    //       Ok(data) => Ok(data),
    //       Err(err) => {
    //         error!("Failed to parse stream message: {}, data: {:?}", err, bytes);
    //         Err(AppResponseError::from(err))
    //       },
    //     }
    //   })
    // });
    // Ok(stream)
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

          let de = StreamDeserializer::new(serde_json::de::SliceRead::new(&this.buffer));
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
                  // Need more data
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

#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
pub struct AppResponseError {
  #[serde(deserialize_with = "default_error_code")]
  pub code: ErrorCode,
  pub message: Cow<'static, str>,
}

impl AppResponseError {
  pub fn new(code: ErrorCode, message: impl Into<Cow<'static, str>>) -> Self {
    Self {
      code,
      message: message.into(),
    }
  }

  pub fn is_record_not_found(&self) -> bool {
    matches!(self.code, ErrorCode::RecordNotFound)
  }
}

impl<T> From<T> for AppResponseError
where
  AppError: std::convert::From<T>,
{
  fn from(value: T) -> Self {
    let err = AppError::from(value);
    Self {
      code: err.code(),
      message: Cow::Owned(err.to_string()),
    }
  }
}

impl Display for AppResponseError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!("code:{:?} msg: {}", self.code, self.message))
  }
}

#[cfg(feature = "cloud")]
impl actix_web::error::ResponseError for AppResponseError {
  fn status_code(&self) -> actix_web::http::StatusCode {
    actix_web::http::StatusCode::OK
  }

  fn error_response(&self) -> actix_web::HttpResponse {
    actix_web::HttpResponse::Ok().json(self)
  }
}

/// Uses a custom deserializer for error codes.
/// This ensures that when a client receives an unrecognized error code, due to version mismatches,
/// it defaults to the `Internal` error code.
fn default_error_code<'a, D: Deserializer<'a>>(deserializer: D) -> Result<ErrorCode, D::Error> {
  match ErrorCode::deserialize(deserializer) {
    Ok(code) => Ok(code),
    Err(_) => Ok(ErrorCode::Internal),
  }
}
