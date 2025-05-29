mod client;
mod database_util;
mod log;
mod user;

// New modules for better organization
mod assertion_utils;
mod async_utils;
mod test_client_config;
mod workspace_ops;

pub use client::*;
pub use log::*;
pub use user::*;

// Export new modules
pub use assertion_utils::*;
pub use async_utils::*;
pub use test_client_config::*;
pub use workspace_ops::*;

#[cfg(not(feature = "v2"))]
mod test_client;

#[cfg(feature = "v2")]
mod test_client_v2;

#[cfg(not(feature = "v2"))]
pub use test_client::*;

#[cfg(feature = "v2")]
pub use test_client_v2::*;
