use std::time::Duration;

mod client;
mod entities;
mod server;

pub use client::*;
pub use server::CollabServer;

pub(crate) const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(8);
pub(crate) const PING_TIMEOUT: Duration = Duration::from_secs(60);
