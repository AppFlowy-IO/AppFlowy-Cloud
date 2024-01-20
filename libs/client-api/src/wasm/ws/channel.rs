use futures::channel::mpsc::{unbounded, UnboundedReceiver, UnboundedSender };
use futures::stream::{Stream, StreamExt};
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::ConnectState;

pub struct WSChannel {
    /// the sender from the channel which is used to handle server message
    pub(crate) message_sender: UnboundedSender<Vec<u8>>,
    pub message_receiver: UnboundedReceiver<Vec<u8>>,
    /// the sender from the channel which is used to handle state change(like connect state change)
    pub(crate) state_sender: UnboundedSender<ConnectState>,
    pub state_receiver: UnboundedReceiver<ConnectState>,
}


pub struct WSReceiver<T> {
    receiver: UnboundedReceiver<T>,
}

impl<T> WSReceiver<T> {
    pub fn new(receiver: UnboundedReceiver<T>) -> Self {
        WSReceiver { receiver }
    }

    pub async fn recv(&mut self) -> Option<T> {
        self.receiver.next().await
    }
}

impl<T> Stream for WSReceiver<T> {
    type Item = T;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>
    ) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.receiver).poll_next(cx)
    }
}

impl Default for WSChannel {
    fn default() -> Self {
        Self::new(100)
    }
}

impl WSChannel {
   pub fn new(buffer_capacity: usize) -> Self {
       let (message_sender, message_receiver) = unbounded();

       let (state_sender, state_receiver) = unbounded();
       Self {
           message_sender,
              message_receiver,
           state_sender,
              state_receiver,
       }
   }

    pub fn send_message(&self, msg: Vec<u8>) {
        let _ = self.message_sender.unbounded_send(msg);
    }

    /// Subscribe the message from server and return a stream
    /// Usage:
    /// ``` text
    /// let channel = WSChannel::new(100);
    /// let mut stream = channel.subscribe_message();
    /// while let Some(msg) = stream.recv().await {
    ///    // handle message
    /// }
    /// ```
    pub fn subscribe_message(&self) -> WSReceiver<Vec<u8>> {
        let (tx, rx) = unbounded();
        wasm_bindgen_futures::spawn_local(async move {
            self.message_receiver.for_each(move |message| {
                let tx = tx.clone();
                async move {
                    let _ = tx.unbounded_send(message).unwrap();
                }
            }).await;
        });

        WSReceiver::new(rx)
    }
}