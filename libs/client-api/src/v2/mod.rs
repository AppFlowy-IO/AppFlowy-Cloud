mod actor;
mod compactor;
mod conn_retry;
mod controller;
mod db;
pub type WorkspaceController = controller::WorkspaceController;
pub type WorkspaceControllerOptions = controller::Options;

pub use actor::ChangedCollab;
pub use controller::ConnectState;
pub use controller::DisconnectedReason;

pub type WorkspaceId = uuid::Uuid;
pub type ObjectId = uuid::Uuid;
