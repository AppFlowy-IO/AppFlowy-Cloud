mod actor;
#[cfg(feature = "collab-sync")]
mod compactor;
mod controller;
mod db;

pub type WorkspaceController = controller::WorkspaceController;
pub type WorkspaceControllerOptions = controller::Options;

pub type WorkspaceId = uuid::Uuid;
pub type ObjectId = uuid::Uuid;
