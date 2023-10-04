use client_api::Client;

mod collab;
mod gotrue;
mod realtime;
mod user;

mod file_storage;

pub const LOCALHOST_URL: &str = "http://localhost:8000";
pub const LOCALHOST_WS: &str = "ws://localhost:8000/ws";
pub const LOCALHOST_GOTRUE: &str = "http://localhost:9998";

pub fn client_api_client() -> Client {
  Client::new(LOCALHOST_URL, LOCALHOST_WS, LOCALHOST_GOTRUE)
}
