use crate::entities::{Editing, RealtimeUser};

use crate::server::collaborate::all_group::AllGroup;
use crate::server::CollabClientStream;
use crate::server::RealtimeAccessControl;

use collab::core::origin::CollabOrigin;
use dashmap::DashMap;
use database::collab::CollabStorage;
use realtime_entity::collab_msg::{ClientCollabMessage, CollabMessage};

use std::collections::HashSet;

use std::sync::Arc;

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

          let client_uid = user.uid();
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
            .client_channel::<CollabMessage, _>(client_uid, object_id, self.access_control.clone());

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
