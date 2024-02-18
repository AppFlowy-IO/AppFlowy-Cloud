use crate::collaborate::group_control::CollabGroupControl;
use crate::collaborate::{CollabAccessControl, CollabClientStream};
use crate::entities::{Editing, RealtimeUser};
use crate::error::StreamError;
use crate::util::channel_ext::UnboundedSenderSink;
use collab::core::origin::CollabOrigin;
use dashmap::DashMap;
use database::collab::CollabStorage;
use realtime_entity::collab_msg::CollabMessage;
use std::collections::HashSet;
use std::future;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, trace, warn};

pub(crate) struct CollabUserMessage<'a, U> {
  pub(crate) user: &'a U,
  pub(crate) collab_message: &'a CollabMessage,
}

pub(crate) struct SubscribeGroup<'a, U, S, AC> {
  pub(crate) message: &'a CollabUserMessage<'a, U>,
  pub(crate) groups: &'a Arc<CollabGroupControl<S, U, AC>>,
  pub(crate) edit_collab_by_user: &'a Arc<DashMap<U, HashSet<Editing>>>,
  pub(crate) client_stream_by_user: &'a Arc<DashMap<U, CollabClientStream>>,
  pub(crate) access_control: &'a Arc<AC>,
}

impl<'a, U, S, AC> SubscribeGroup<'a, U, S, AC>
where
  U: RealtimeUser,
  S: CollabStorage,
  AC: CollabAccessControl,
{
  fn get_origin(collab_message: &CollabMessage) -> &CollabOrigin {
    collab_message.origin().unwrap_or_else(|| {
      error!("🔴The origin from client message is empty");
      &CollabOrigin::Empty
    })
  }

  fn make_channel<'b>(
    object_id: &'b str,
    client_stream: &'b mut CollabClientStream,
    client_uid: i64,
    sink_permission_service: Arc<AC>,
    stream_permission_service: Arc<AC>,
  ) -> (
    UnboundedSenderSink<CollabMessage>,
    ReceiverStream<Result<CollabMessage, StreamError>>,
  )
  where
    'a: 'b,
  {
    let (sink, stream) = client_stream.client_channel::<CollabMessage, _, _>(
      object_id,
      move |object_id, msg| {
        if msg.object_id() != object_id {
          warn!(
            "The object id:{} from message is not matched with the object id:{} from sink",
            msg.object_id(),
            object_id
          );
          return Box::pin(future::ready(false));
        }

        let object_id = object_id.to_string();
        let cloned_sink_permission_service = sink_permission_service.clone();
        Box::pin(async move {
          match cloned_sink_permission_service
            .can_receive_collab_update(&client_uid, &object_id)
            .await
          {
            Ok(is_allowed) => {
              if !is_allowed {
                trace!(
                  "user:{} is not allowed to receive {} updates",
                  client_uid,
                  object_id,
                );
              }

              is_allowed
            },
            Err(err) => {
              trace!(
                "user:{} fail to receive updates by error: {}",
                client_uid,
                err
              );
              false
            },
          }
        })
      },
      move |object_id, msg| {
        if msg.object_id() != object_id {
          return Box::pin(future::ready(false));
        }

        let is_init = msg.is_client_init();
        let object_id = object_id.to_string();
        let cloned_stream_permission_service = stream_permission_service.clone();

        Box::pin(async move {
          // If the message is init sync, and it's allow the send to the group.
          if is_init {
            return true;
          }

          match cloned_stream_permission_service
            .can_send_collab_update(&client_uid, &object_id)
            .await
          {
            Ok(is_allowed) => {
              if !is_allowed {
                trace!(
                  "client:{} is not allowed to send {} updates",
                  client_uid,
                  object_id,
                );
              }
              is_allowed
            },
            Err(err) => {
              trace!(
                "client:{} can't  send update with error: {}",
                client_uid,
                err
              );
              false
            },
          }
        })
      },
    );
    (sink, stream)
  }
}

impl<'a, U, S, AC> SubscribeGroup<'a, U, S, AC>
where
  U: RealtimeUser,
  S: CollabStorage,
  AC: CollabAccessControl,
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

          let (sink, stream) = Self::make_channel(
            object_id,
            client_stream.value_mut(),
            client_uid,
            self.access_control.clone(),
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
