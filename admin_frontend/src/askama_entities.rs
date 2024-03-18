use database_entity::dto::AFWorkspace;

use crate::ext::entities::WorkspaceMember;

pub struct WorkspaceWithMembers {
  pub workspace: AFWorkspace,
  pub members: Vec<WorkspaceMember>,
}
