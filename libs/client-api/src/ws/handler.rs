use crate::af_spawn;
use collab_rt_entity::ClientCollabMessage;
use collab_rt_entity::RealtimeMessage;
use futures_util::Sink;
use std::fmt::Debug;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::broadcast::{channel, Sender};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{trace, warn};

pub struct WebSocketChannel<T> {
  #[allow(dead_code)]
  object_id: String,
  rt_msg_sender: Sender<Vec<ClientCollabMessage>>,
  receiver: Sender<T>,
}

impl<T> Drop for WebSocketChannel<T> {
  fn drop(&mut self) {
    #[cfg(feature = "sync_verbose_log")]
    trace!("Drop WebSocketChannel {}", self.object_id);
  }
}

impl<T> WebSocketChannel<T>
where
  T: Into<RealtimeMessage> + Clone + Send + Sync + 'static,
{
  pub fn new(object_id: &str, rt_msg_sender: Sender<Vec<ClientCollabMessage>>) -> Self {
    let object_id = object_id.to_string();
    let (receiver, _) = channel(1000);
    Self {
      object_id,
      rt_msg_sender,
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

  /// Use to send message to server via WebSocket.
  pub fn sink(&self) -> BroadcastSink<Vec<ClientCollabMessage>> {
    let (tx, mut rx) = unbounded_channel::<Vec<ClientCollabMessage>>();
    let cloned_sender = self.rt_msg_sender.clone();
    let object_id = self.object_id.clone();
    af_spawn(async move {
      while let Some(msg) = rx.recv().await {
        let _ = cloned_sender.send(msg);
      }

      trace!("WebSocketChannel {} sink closed", object_id);
    });
    BroadcastSink::new(tx)
  }

  /// Use to receive message from server via WebSocket.
  pub fn stream(&self) -> UnboundedReceiverStream<Result<T, anyhow::Error>> {
    let (tx, rx) = unbounded_channel::<Result<T, anyhow::Error>>();
    let mut recv = self.receiver.subscribe();
    let object_id = self.object_id.clone();
    af_spawn(async move {
      while let Ok(msg) = recv.recv().await {
        if let Err(err) = tx.send(Ok(msg)) {
          trace!("Failed to send message to channel stream: {}", err);
          break;
        }
      }
      if cfg!(feature = "sync_verbose_log") {
        trace!("WebSocketChannel {} stream closed", object_id);
      }
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
