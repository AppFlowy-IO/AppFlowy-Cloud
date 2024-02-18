use crate::collaborate::CollabClientStream;

use anyhow::{anyhow, Error};
use collab::core::origin::CollabOrigin;
use database::collab::CollabStorage;
use futures_util::SinkExt;

use realtime_entity::collab_msg::{CollabMessage, CollabSinkMessage};

use dashmap::DashMap;
use std::collections::hash_map::Entry;
use std::collections::HashSet;
use std::future;
use std::future::Future;
use std::iter::Take;
use std::pin::Pin;
use std::sync::{Arc, Weak};
use std::time::Duration;

use crate::entities::{Editing, RealtimeUser};
use tokio_retry::strategy::FixedInterval;
use tokio_retry::{Action, Condition, Retry, RetryIf};
use tokio_stream::wrappers::ReceiverStream;

use crate::collaborate::group::CollabGroupCache;
use crate::collaborate::permission::CollabAccessControl;
use crate::error::{RealtimeError, StreamError};
use crate::util::channel_ext::UnboundedSenderSink;
use tracing::{error, trace, warn};

pub(crate) struct CollabUserMessage<'a, U> {
  pub(crate) user: &'a U,
  pub(crate) collab_message: &'a CollabMessage,
}

pub(crate) struct SubscribeGroupIfNeed<'a, U, S, AC> {
  pub(crate) collab_user_message: &'a CollabUserMessage<'a, U>,
  pub(crate) groups: &'a Arc<CollabGroupCache<S, U, AC>>,
  pub(crate) edit_collab_by_user: &'a Arc<DashMap<U, HashSet<Editing>>>,
  pub(crate) client_stream_by_user: &'a Arc<DashMap<U, CollabClientStream>>,
  pub(crate) access_control: &'a Arc<AC>,
}

impl<'a, U, S, AC> SubscribeGroupIfNeed<'a, U, S, AC>
where
  U: RealtimeUser,
  S: CollabStorage,
  AC: CollabAccessControl,
{
  pub(crate) fn run(
    self,
  ) -> RetryIf<Take<FixedInterval>, SubscribeGroupIfNeed<'a, U, S, AC>, SubscribeGroupCondition<U>>
  {
    let weak_client_stream = Arc::downgrade(self.client_stream_by_user);
    let retry_strategy = FixedInterval::new(Duration::from_secs(2)).take(5);
    RetryIf::spawn(
      retry_strategy,
      self,
      SubscribeGroupCondition(weak_client_stream),
    )
  }
}

impl<'a, U, S, AC> Action for SubscribeGroupIfNeed<'a, U, S, AC>
where
  U: RealtimeUser,
  S: CollabStorage,
  AC: CollabAccessControl,
{
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + 'a>>;
  type Item = ();
  type Error = RealtimeError;

  fn run(&mut self) -> Self::Future {
    Box::pin(async {
      let CollabUserMessage {
        user,
        collab_message,
      } = self.collab_user_message;

      let object_id = collab_message.object_id();
      if !self.groups.contains_group(object_id).await? {
        if collab_message.is_init_msg() {
          let groups = self.groups.clone();
          Self::create_new_group(&groups, collab_message, object_id).await?;
        } else {
          // If the collab message is not init sync. Discard it.
          return Ok(());
        }
      }

      // Where an "init sync message" is received, it typically indicates that the client has reopened
      // the collaboration session. Such a situation can arise due to network issues leading to websocket reconnection.
      // 1. The user is first removed from the group to reset their state in the session.
      // 2. Then, the client's stream is subscribed to the group again, effectively re-establishing the user's participation in the collaborative session.
      if collab_message.is_init_msg() {
        self.groups.remove_user(object_id, user).await?;
      } else if self
        .groups
        .contains_user(object_id, user)
        .await
        .unwrap_or(false)
      {
        // In this case, it implies that the client is already a member of the group.
        // Therefore, no further action is required.
        return Ok(());
      }

      let origin = Self::get_origin(collab_message);
      if let Some(mut client_stream) = self.client_stream_by_user.get_mut(user) {
        if let Some(collab_group) = self.groups.get_group(object_id).await {
          if let Entry::Vacant(entry) = collab_group
            .subscribers
            .try_write()
            .map_err(|err| RealtimeError::Internal(err.into()))?
            .entry((*user).clone())
          {
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

            entry.insert(collab_group.subscribe(origin.clone(), sink, stream));
          }
        }
      } else {
        warn!("The client stream: {} is not found", user);
      }
      Ok(())
    })
  }
}

impl<'a, U, S, AC> SubscribeGroupIfNeed<'a, U, S, AC>
where
  U: RealtimeUser,
  S: CollabStorage,
  AC: CollabAccessControl,
{
  async fn create_new_group(
    groups: &Arc<CollabGroupCache<S, U, AC>>,
    collab_message: &CollabMessage,
    object_id: &str,
  ) -> Result<(), RealtimeError> {
    match collab_message {
      CollabMessage::ClientInitSync(client_init) => {
        let uid = client_init
          .origin
          .client_user_id()
          .ok_or(RealtimeError::UnexpectedData("The client user id is empty"))?;
        groups
          .create_group_if_need(
            uid,
            &client_init.workspace_id,
            object_id,
            client_init.collab_type.clone(),
          )
          .await;

        Ok(())
      },
      _ => Err(RealtimeError::UnexpectedData(
        "The first message must be init sync message",
      )),
    }
  }

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
                error!(
                  "user:{} is not allowed to receive {} updates",
                  client_uid, object_id,
                );
              }

              is_allowed
            },
            Err(err) => {
              error!(
                "user:{} fail to receive updates by error: {}",
                client_uid, err
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
                error!(
                  "client:{} is not allowed to send {} updates",
                  client_uid, object_id,
                );
              }
              is_allowed
            },
            Err(err) => {
              error!(
                "client:{} can't  send update with error: {}",
                client_uid, err
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

pub struct SubscribeGroupCondition<U>(pub Weak<DashMap<U, CollabClientStream>>);
impl<U> Condition<RealtimeError> for SubscribeGroupCondition<U> {
  fn should_retry(&mut self, _error: &RealtimeError) -> bool {
    self.0.upgrade().is_some()
  }
}

pub struct SinkCollabMessageAction<'a, Sink> {
  pub sink: &'a Arc<tokio::sync::Mutex<Sink>>,
  pub message: CollabMessage,
}

impl<'a, Sink> SinkCollabMessageAction<'a, Sink>
where
  Sink: SinkExt<CollabMessage> + Send + Sync + Unpin + 'a,
{
  pub fn run(self) -> Retry<Take<FixedInterval>, SinkCollabMessageAction<'a, Sink>> {
    let retry_strategy = FixedInterval::new(Duration::from_secs(2)).take(5);
    Retry::spawn(retry_strategy, self)
  }
}

impl<'a, Sink> Action for SinkCollabMessageAction<'a, Sink>
where
  Sink: SinkExt<CollabMessage> + Send + Sync + Unpin + 'a,
{
  type Future = Pin<Box<dyn Future<Output = Result<Self::Item, Self::Error>> + Send + Sync + 'a>>;
  type Item = ();
  type Error = RealtimeError;

  fn run(&mut self) -> Self::Future {
    let sink = self.sink.clone();
    let message = self.message.clone();
    Box::pin(async move {
      let mut sink = sink
        .try_lock()
        .map_err(|err| RealtimeError::Internal(Error::from(err)))?;
      sink
        .send(message)
        .await
        .map_err(|_err| RealtimeError::Internal(anyhow!("Sink message fail")))?;
      Ok(())
    })
  }
}
