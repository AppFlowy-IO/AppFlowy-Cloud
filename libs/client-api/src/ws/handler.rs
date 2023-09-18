use crate::ws::{BusinessID, ClientRealtimeMessage, WSError};
use futures_util::Sink;
use std::fmt::Debug;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::broadcast::{channel, Sender};
use tokio::sync::mpsc::{unbounded_channel, UnboundedSender};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_tungstenite::tungstenite::Message;

pub struct WebSocketChannel {
  business_id: BusinessID,
  sender: Sender<Message>,
  receiver: Sender<ClientRealtimeMessage>,
}

impl WebSocketChannel {
  pub fn new(business_id: BusinessID, sender: Sender<Message>) -> Self {
    let (receiver, _) = channel(1000);
    Self {
      business_id,
      sender,
      receiver,
    }
  }

  pub fn business_id(&self) -> &BusinessID {
    &self.business_id
  }

  pub(crate) fn recv_msg(&self, msg: &ClientRealtimeMessage) {
    let _ = self.receiver.send(msg.clone());
  }

  pub fn sink<T>(&self) -> BroadcastSink<T>
  where
    T: Into<ClientRealtimeMessage> + Send + Sync + 'static + Clone,
  {
    let (tx, mut rx) = unbounded_channel::<T>();
    let cloned_sender = self.sender.clone();
    tokio::spawn(async move {
      while let Some(msg) = rx.recv().await {
        let ws_msg: ClientRealtimeMessage = msg.into();
        match cloned_sender.send(ws_msg.into()) {
          Ok(_) => {},
          Err(e) => tracing::error!("🔴Error sending message: {:?}", e),
        }
      }
    });
    BroadcastSink::new(tx)
  }

  pub fn stream<T>(&self) -> UnboundedReceiverStream<T>
  where
    T: From<ClientRealtimeMessage> + Send + Sync + 'static,
  {
    let (tx, rx) = unbounded_channel::<T>();
    let mut recv = self.receiver.subscribe();
    tokio::spawn(async move {
      while let Ok(msg) = recv.recv().await {
        let _ = tx.send(T::from(msg));
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
  type Error = WSError;

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
