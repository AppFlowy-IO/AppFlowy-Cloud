use crate::platform_spawn;
use futures_util::Sink;
use realtime_entity::message::RealtimeMessage;
use std::fmt::Debug;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::broadcast::{channel, Sender};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{trace, warn};
use websocket::Message;

pub struct WebSocketChannel<T> {
  object_id: String,
  sender: Sender<Message>,
  receiver: Sender<T>,
}

impl<T> Drop for WebSocketChannel<T> {
  fn drop(&mut self) {
    trace!("Drop WebSocketChannel {}", self.object_id);
  }
}

impl<T> WebSocketChannel<T>
where
  T: Into<RealtimeMessage> + Clone + Send + Sync + 'static,
{
  pub fn new(object_id: &str, sender: Sender<Message>) -> Self {
    let object_id = object_id.to_string();
    let (receiver, _) = channel(1000);
    Self {
      object_id,
      sender,
      receiver,
    }
  }

  /// Forward message to the stream returned by [WebSocketChannel::stream] method.
  /// Calling this method to forward the server message to the receiver stream.
  pub(crate) fn forward_to_stream(&self, msg: T) {
    if let Err(err) = self.receiver.send(msg) {
      warn!("Failed to send message to channel: {}", err);
    }
  }

  pub fn sink(&self) -> BroadcastSink<T> {
    let (tx, mut rx) = unbounded_channel::<T>();
    let cloned_sender = self.sender.clone();
    let object_id = self.object_id.clone();
    platform_spawn(async move {
      while let Some(msg) = rx.recv().await {
        let realtime_msg: RealtimeMessage = msg.into();
        let _ = cloned_sender.send(realtime_msg.into());
      }
      trace!("WebSocketChannel {} sink closed", object_id);
    });
    BroadcastSink::new(tx)
  }

  pub fn stream(&self) -> UnboundedReceiverStream<Result<T, anyhow::Error>> {
    let (tx, rx) = unbounded_channel::<Result<T, anyhow::Error>>();
    let mut recv = self.receiver.subscribe();
    let object_id = self.object_id.clone();
    platform_spawn(async move {
      while let Ok(msg) = recv.recv().await {
        if let Err(err) = tx.send(Ok(msg)) {
          trace!("Failed to send message to channel stream: {}", err);
          break;
        }
      }
      trace!("WebSocketChannel {} stream closed", object_id);
    });
    UnboundedReceiverStream::new(rx)
  }
}

pub struct BroadcastSink<T>(pub UnboundedSender<T>);

impl<T> BroadcastSink<T> {
  pub fn new(tx: UnboundedSender<T>) -> Self {
    Self(tx)
  }
}

impl<T> Sink<T> for BroadcastSink<T>
where
  T: Send + Sync + 'static + Debug,
{
  type Error = anyhow::Error;

  fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
    let _ = self.0.send(item);
    Ok(())
  }

  fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }

  fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
    Poll::Ready(Ok(()))
  }
}
