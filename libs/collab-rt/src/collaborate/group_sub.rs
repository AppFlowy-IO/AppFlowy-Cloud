use crate::collaborate::all_group::AllGroup;
use crate::CollabClientStream;
use crate::RealtimeAccessControl;

use collab::core::origin::CollabOrigin;
use collab_rt_entity::collab_msg::{ClientCollabMessage, CollabMessage};
use dashmap::DashMap;
use database::collab::CollabStorage;

use std::collections::HashSet;

use std::sync::Arc;

use collab_rt_entity::user::{Editing, RealtimeUser};
use tracing::{trace, warn};

pub(crate) struct CollabUserMessage<'a, U> {
  pub(crate) user: &'a U,
  pub(crate) collab_message: &'a ClientCollabMessage,
}

pub(crate) struct SubscribeGroup<'a, S, U, AC> {
  pub(crate) message: &'a CollabUserMessage<'a, U>,
  pub(crate) groups: &'a Arc<AllGroup<S, U, AC>>,
  pub(crate) edit_collab_by_user: &'a Arc<DashMap<U, HashSet<Editing>>>,
  pub(crate) client_stream_by_user: &'a Arc<DashMap<U, CollabClientStream>>,
  pub(crate) access_control: &'a Arc<AC>,
}

impl<'a, S, U, AC> SubscribeGroup<'a, S, U, AC>
where
  U: RealtimeUser,
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  fn get_origin(collab_message: &ClientCollabMessage) -> &CollabOrigin {
    collab_message.origin()
  }
}

impl<'a, S, U, AC> SubscribeGroup<'a, S, U, AC>
where
  U: RealtimeUser,
  S: CollabStorage,
  AC: RealtimeAccessControl,
{
  pub(crate) async fn run(self) {
    let CollabUserMessage {
      user,
      collab_message,
    } = self.message;

    let object_id = collab_message.object_id();
    let origin = Self::get_origin(collab_message);
    if let Some(mut client_stream) = self.client_stream_by_user.get_mut(user) {
      if let Some(collab_group) = self.groups.get_group(object_id).await {
        if !collab_group.contains_user(user) {
          trace!(
            "[realtime]: {} subscribe group:{}",
            user,
            collab_message.object_id()
          );

          self
            .edit_collab_by_user
            .entry((*user).clone())
            .or_default()
            .insert(Editing {
              object_id: object_id.to_string(),
              origin: origin.clone(),
            });

          let (sink, stream) = client_stream
            .value_mut()
            .client_channel::<CollabMessage, _, U>(
              &collab_group.workspace_id,
              user,
              object_id,
              self.access_control.clone(),
            );

          collab_group
            .subscribe(user, origin.clone(), sink, stream)
            .await;
        }
      }
    } else {
      warn!("The client stream: {} is not found", user);
    }
  }
}
