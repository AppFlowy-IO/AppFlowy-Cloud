use futures_util::SinkExt;
use web_sys::{WebSocket, BinaryType, MessageEvent, ErrorEvent, CloseEvent};
use web_sys::js_sys::Uint8Array;
use web_sys::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::JsCast;
use crate::{WSError};
use crate::wasm::{Heartbeat, WSChannel, WSReceiver};


pub struct WSConfig {
    pub url: String,
    pub buffer_capacity: usize,
}

#[derive(Debug, Default)]
pub struct WSClient {
    ws: Option<WebSocket>,
    heartbeat: Option<Heartbeat>,
    channel: WSChannel,
}

const INTERVAL_MS: i32 = 1000;
impl WSClient {
    pub fn new(config: WSConfig) -> WSClient {
        let channel = WSChannel::new(config.buffer_capacity);

        WSClient {
            ws: None,
            heartbeat: None,
            channel,
        }
    }

    pub fn connect(&mut self, url: &str) -> Result<(), WSError> {
        let ws = WebSocket::new(url).map_err(|e| WSError::from(e))?;

        // Set the binary type to ArrayBuffer.
        ws.set_binary_type(BinaryType::Arraybuffer);

        // Setup callbacks for handling messages.
        let cloned_ws = ws.clone();
        let onopen = Closure::wrap(Box::new(move |_| {
            self.onopen();
        }) as Box<dyn FnMut(_)>);
        // set onopen callback
        cloned_ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        // forget the closure to keep it alive
        onopen.forget();

        let onmessage = Closure::wrap(Box::new(move |e: MessageEvent| {
            // receive message from the server
            let data = e.data().into_array_buffer().unwrap();
            let array = Uint8Array::new(&data);
            let vec = array.to_vec();
            self.handle_receive_from_server_message(vec);
        }) as Box<dyn FnMut(_)>);
        // set onmessage callback
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();

        let onerror = Closure::wrap(Box::new(move |e: ErrorEvent| {
            // handle error
        }) as Box<dyn FnMut(_)>);
        // set onerror callback
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();

        let onclose = Closure::wrap(Box::new(move |e: CloseEvent| {
            // handle close
        }) as Box<dyn FnMut(_)>);

        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();

        self.ws = Some(ws);
        Ok(())
    }

    pub fn disconnect(&mut self) {
        if let Some(ref ws) = self.ws {
            ws.close().unwrap();
        }
        self.ws = None;
    }


    pub fn send(&self, message: &Vec<u8>) -> Result<(), WSError> {
        if let Some(ref ws) = self.ws {
            ws.send_with_u8_array(message).map_err(|e| {
                // If send message failed, try to reconnect to the server and send again
                // If reconnect failed, try to send http request to the server
                WSError::from(e)
            })
        } else {
            // If there is no websocket connection, try to reconnect to the server and send again
            WSError::LostConnection("No websocket connection".to_string())
        }
    }

    pub fn subscribe_message(&self) -> WSReceiver<Vec<u8>> {
        self.channel.subscribe_message()
    }

    fn send_heartbeat(&self) {
        if let Some(ref ws) = self.ws {
            if let Err(e) = ws.send_with_str("Heartbeat message") {
                // Handle error, e.g., log it or close the connection
            }
        }
    }

    fn onopen(&mut self) {
        // Start heartbeats
        let mut heartbeat = Heartbeat::new(INTERVAL_MS);
        heartbeat.start(move || {
            self.send_heartbeat();
        });

        self.heartbeat = Some(heartbeat); // Store the heartbeat instance in WSClient
    }

    fn handle_receive_from_server_message(&self, message: Vec<u8>) {
        if let Err(error) = self.channel.message_sender.unbounded_send(message) {
            // Handle error, e.g., log it or close the connection
        }
    }


}
