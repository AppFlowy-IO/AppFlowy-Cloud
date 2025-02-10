use crate::ws2::{ConnectionError, ObjectId, WorkspaceId};
use collab::core::collab::CollabContext;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;

pub(super) struct CollabCache {
  workspace_id: WorkspaceId,
  active: DashMap<ObjectId, CollabContext>,
}

impl CollabCache {
  pub fn new(workspace_id: WorkspaceId) -> Self {
    CollabCache {
      workspace_id,
      active: DashMap::new(),
    }
  }

  pub fn get(&self, object_id: ObjectId) -> Result<&CollabContext, ConnectionError> {
    let entry = self.active.entry(object_id);
    match entry {
      Entry::Occupied(e) => Ok(e.get()),
      Entry::Vacant(e) => {
        todo!()
      },
    }
  }

  pub fn get_mut(&self, object_id: ObjectId) -> Result<&mut CollabContext, ConnectionError> {
    let entry = self.active.entry(object_id);
    match entry {
      Entry::Occupied(mut e) => Ok(e.get_mut()),
      Entry::Vacant(e) => {
        todo!()
      },
    }
  }

  pub fn remove(&self, object_id: &ObjectId) -> Result<(), ConnectionError> {
    self.active.remove(object_id);
    //TODO: remove from persistent store
    Ok(())
  }
}
