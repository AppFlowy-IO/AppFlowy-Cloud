use crate::RealtimeClientWebsocketSink;
use crate::error::RealtimeError;
use collab_rt_entity::RealtimeMessage;
use futures::Sink;
use std::marker::PhantomData;
use std::pin::Pin;
use std::task::{Context, Poll};

/// Futures [Sink] compatible sender, that always throws the input away.
/// Essentially: a `/dev/null` equivalent.
#[derive(Clone)]
pub(crate) struct NullSender<T> {
  _marker: PhantomData<T>,
}

impl<T> Default for NullSender<T> {
  fn default() -> Self {
    NullSender {
      _marker: PhantomData,
    }
  }
}

impl<T> Sink<T> for NullSender<T> {
  type Error = RealtimeError;

  #[inline]
  fn poll_ready(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  #[inline]
  fn start_send(self: Pin<&mut Self>, _: T) -> Result<(), Self::Error> {
    Ok(())
  }

  #[inline]
  fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  #[inline]
  fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }
}

impl<T> RealtimeClientWebsocketSink for NullSender<T>
where
  T: Send + Sync + 'static,
{
  fn do_send(&self, _message: RealtimeMessage) {}
}
