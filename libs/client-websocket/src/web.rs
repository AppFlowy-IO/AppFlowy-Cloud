use http::HeaderMap;
use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
use std::{cell::RefCell, collections::VecDeque, rc::Rc, task::Waker};
use wasm_bindgen::{closure::Closure, JsCast};
use web_sys::{CloseEvent, ErrorEvent, MessageEvent, WebSocket};

pub async fn connect_async(url: &str, header_map: HeaderMap) -> crate::Result<WebSocketStream> {
  WebSocketStream::new(url, header_map).await
}

pub struct WebSocketStream {
  inner: WebSocket,
  queue: Rc<RefCell<VecDeque<crate::Result<crate::Message>>>>,
  waker: Rc<RefCell<Option<Waker>>>,
  _on_message_callback: Closure<dyn FnMut(MessageEvent)>,
  _on_error_callback: Closure<dyn FnMut(ErrorEvent)>,
  _on_close_callback: Closure<dyn FnMut(CloseEvent)>,
}

impl WebSocketStream {
  async fn new(url: &str, headers: HeaderMap) -> crate::Result<Self> {
    let query_string = header_map_to_query_string(&headers);
    // Construct the full WebSocket URL with query parameters
    let conn_url = format!("{}?{}", url, query_string);
    match web_sys::WebSocket::new(&conn_url) {
      Err(_err) => Err(crate::Error::Url(
        crate::error::UrlError::UnsupportedUrlScheme,
      )),
      Ok(ws) => {
        ws.set_binary_type(web_sys::BinaryType::Arraybuffer);

        let (open_sx, open_rx) = futures_channel::oneshot::channel();
        let on_open_callback = {
          let mut open_sx = Some(open_sx);
          Closure::wrap(Box::new(move |_event| {
            open_sx.take().map(|open_sx| open_sx.send(()));
          }) as Box<dyn FnMut(web_sys::Event)>)
        };
        ws.set_onopen(Some(on_open_callback.as_ref().unchecked_ref()));

        let (err_sx, err_rx) = futures_channel::oneshot::channel();
        let on_error_callback = {
          let mut err_sx = Some(err_sx);
          Closure::wrap(Box::new(move |_error_event| {
            err_sx.take().map(|err_sx| err_sx.send(()));
          }) as Box<dyn FnMut(ErrorEvent)>)
        };
        ws.set_onerror(Some(on_error_callback.as_ref().unchecked_ref()));

        let result = futures_util::future::select(open_rx, err_rx).await;
        ws.set_onopen(None);
        ws.set_onerror(None);
        let ws = match result {
          futures_util::future::Either::Left((_, _)) => Ok(ws),
          futures_util::future::Either::Right((_, _)) => Err(crate::Error::ConnectionClosed),
        }?;

        let waker = Rc::new(RefCell::new(Option::<Waker>::None));
        let queue = Rc::new(RefCell::new(VecDeque::new()));
        let on_message_callback = {
          let waker = Rc::clone(&waker);
          let queue = Rc::clone(&queue);
          Closure::wrap(Box::new(move |event: MessageEvent| {
            let payload = std::convert::TryFrom::try_from(event);
            queue.borrow_mut().push_back(payload);
            if let Some(waker) = waker.borrow_mut().take() {
              waker.wake();
            }
          }) as Box<dyn FnMut(MessageEvent)>)
        };
        ws.set_onmessage(Some(on_message_callback.as_ref().unchecked_ref()));

        let on_error_callback = {
          let waker = Rc::clone(&waker);
          let queue = Rc::clone(&queue);
          Closure::wrap(Box::new(move |_error_event| {
            queue
              .borrow_mut()
              .push_back(Err(crate::Error::ConnectionClosed));
            if let Some(waker) = waker.borrow_mut().take() {
              waker.wake();
            }
          }) as Box<dyn FnMut(ErrorEvent)>)
        };
        ws.set_onerror(Some(on_error_callback.as_ref().unchecked_ref()));

        let on_close_callback = {
          let waker = Rc::clone(&waker);
          let queue = Rc::clone(&queue);
          Closure::wrap(Box::new(move |event: CloseEvent| {
            queue.borrow_mut().push_back(Ok(crate::Message::Close(Some(
              crate::message::CloseFrame {
                code: event.code().into(),
                reason: event.reason().into(),
              },
            ))));
            if let Some(waker) = waker.borrow_mut().take() {
              waker.wake();
            }
          }) as Box<dyn FnMut(CloseEvent)>)
        };
        ws.set_onclose(Some(on_close_callback.as_ref().unchecked_ref()));

        Ok(Self {
          inner: ws,
          queue,
          waker,
          _on_message_callback: on_message_callback,
          _on_error_callback: on_error_callback,
          _on_close_callback: on_close_callback,
        })
      },
    }
  }
}

impl Drop for WebSocketStream {
  fn drop(&mut self) {
    let _r = self.inner.close();
    self.inner.set_onmessage(None);
    self.inner.set_onclose(None);
    self.inner.set_onerror(None);
  }
}

enum ReadyState {
  Closed,
  Closing,
  Connecting,
  Open,
}

impl std::convert::TryFrom<u16> for ReadyState {
  type Error = ();

  fn try_from(value: u16) -> Result<Self, Self::Error> {
    match value {
      web_sys::WebSocket::CLOSED => Ok(Self::Closed),
      web_sys::WebSocket::CLOSING => Ok(Self::Closing),
      web_sys::WebSocket::OPEN => Ok(Self::Open),
      web_sys::WebSocket::CONNECTING => Ok(Self::Connecting),
      _ => Err(()),
    }
  }
}

mod stream {
  use super::ReadyState;
  use std::pin::Pin;
  use std::task::{Context, Poll};

  impl futures_util::Stream for super::WebSocketStream {
    type Item = crate::Result<crate::Message>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
      if self.queue.borrow().is_empty() {
        *self.waker.borrow_mut() = Some(cx.waker().clone());

        match std::convert::TryFrom::try_from(self.inner.ready_state()) {
          Ok(ReadyState::Open) => Poll::Pending,
          _ => None.into(),
        }
      } else {
        self.queue.borrow_mut().pop_front().into()
      }
    }
  }

  impl futures_util::Sink<crate::Message> for super::WebSocketStream {
    type Error = crate::Error;

    fn poll_ready(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      match std::convert::TryFrom::try_from(self.inner.ready_state()) {
        Ok(ReadyState::Open) => Ok(()).into(),
        _ => Err(crate::Error::ConnectionClosed).into(),
      }
    }

    fn start_send(self: Pin<&mut Self>, item: crate::Message) -> Result<(), Self::Error> {
      match std::convert::TryFrom::try_from(self.inner.ready_state()) {
        Ok(ReadyState::Open) => {
          match item {
            crate::Message::Text(text) => self
              .inner
              .send_with_str(&text)
              .map_err(|_| crate::Error::Utf8)?,
            crate::Message::Binary(bin) => self
              .inner
              .send_with_u8_array(&bin)
              .map_err(|_| crate::Error::Utf8)?,
            crate::Message::Close(frame) => match frame {
              None => self
                .inner
                .close()
                .map_err(|_| crate::Error::AlreadyClosed)?,
              Some(frame) => self
                .inner
                .close_with_code_and_reason(frame.code.into(), &frame.reason)
                .map_err(|_| crate::Error::AlreadyClosed)?,
            },
            crate::Message::Ping(data) => self
              .inner
              .send_with_u8_array(&data)
              .map_err(|_| crate::Error::Utf8)?,
            crate::Message::Pong(data) => self
              .inner
              .send_with_u8_array(&data)
              .map_err(|_| crate::Error::Utf8)?,
          }
          Ok(())
        },
        _ => Err(crate::Error::ConnectionClosed),
      }
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      Ok(()).into()
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
      self
        .inner
        .close()
        .map_err(|_| crate::Error::AlreadyClosed)?;
      Ok(()).into()
    }
  }
}

impl std::convert::TryFrom<web_sys::MessageEvent> for crate::Message {
  type Error = crate::Error;

  fn try_from(event: MessageEvent) -> Result<Self, Self::Error> {
    match event.data() {
      payload if payload.is_instance_of::<js_sys::ArrayBuffer>() => {
        let buffer = js_sys::Uint8Array::new(payload.unchecked_ref());
        let mut v = vec![0; buffer.length() as usize];
        buffer.copy_to(&mut v);
        Ok(crate::Message::Binary(v))
      },
      payload if payload.is_string() => match payload.as_string() {
        Some(text) => Ok(crate::Message::Text(text)),
        None => Err(crate::Error::Utf8),
      },
      payload if payload.is_instance_of::<web_sys::Blob>() => {
        Err(crate::Error::BlobFormatUnsupported)
      },
      _ => Err(crate::Error::UnknownFormat),
    }
  }
}

fn header_map_to_query_string(headers: &HeaderMap) -> String {
  headers
    .iter()
    .filter_map(|(name, value)| {
      // Convert the header name and value to string
      let name = name.as_str();
      let value = value.to_str().ok()?;
      Some((name, value))
    })
    .map(|(name, value)| {
      // Percent-encode the name and value to ensure they are URL-safe
      let name_encoded = utf8_percent_encode(name, NON_ALPHANUMERIC).to_string();
      let value_encoded = utf8_percent_encode(value, NON_ALPHANUMERIC).to_string();
      format!("{}={}", name_encoded, value_encoded)
    })
    .collect::<Vec<_>>()
    .join("&")
}
