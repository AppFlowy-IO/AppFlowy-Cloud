use crate::client::constants::{LOCALHOST_URL, LOCALHOST_WS};
use client::constants::LOCALHOST_GOTRUE;
use client_api::Client;

mod client;
mod collab;
mod gotrue;
mod realtime;

pub fn client_api_client() -> Client {
  Client::new(LOCALHOST_URL, LOCALHOST_WS, LOCALHOST_GOTRUE)
}
