use crate::error::RealtimeError;
use crate::group::database_init::DatabaseCollabGroup;
use crate::group::group_init::DefaultCollabGroup;
use actix::dev::Stream;
use collab::core::origin::CollabOrigin;
use collab::entity::EncodedCollab;
use collab_rt_entity::user::RealtimeUser;
use collab_rt_entity::{CollabMessage, MessageByObjectId};
use futures::Sink;

#[derive(Clone)]
pub enum CollabGroup {
  Default(DefaultCollabGroup),
  Database(DatabaseCollabGroup),
}

impl CollabGroup {
  pub async fn subscribe<Sink, Stream>(
    &self,
    user: &RealtimeUser,
    subscriber_origin: CollabOrigin,
    sink: Sink,
    stream: Stream,
  ) where
    Sink: SubscriptionSink + Clone + 'static,
    Stream: SubscriptionStream + 'static,
  {
    match self {
      CollabGroup::Default(group) => group.subscribe(user, subscriber_origin, sink, stream),
      CollabGroup::Database(group) => group.subscribe(user, subscriber_origin, sink, stream).await,
    }
  }

  pub async fn encode_collab(&self) -> Result<EncodedCollab, RealtimeError> {
    match self {
      CollabGroup::Default(group) => group.encode_collab().await,
      CollabGroup::Database(group) => group.encode_collab().await,
    }
  }

  pub fn contains_user(&self, user: &RealtimeUser) -> bool {
    match self {
      CollabGroup::Default(group) => group.contains_user(user),
      CollabGroup::Database(group) => group.contains_user(user),
    }
  }

  pub fn remove_user(&self, user: &RealtimeUser) {
    match self {
      CollabGroup::Default(group) => group.remove_user(user),
      CollabGroup::Database(group) => group.remove_user(user),
    }
  }

  /// Check if the group is active. A group is considered active if it has at least one
  /// subscriber
  pub fn is_inactive(&self) -> bool {
    match self {
      CollabGroup::Default(group) => group.is_inactive(),
      CollabGroup::Database(group) => group.is_inactive(),
    }
  }

  pub fn workspace_id(&self) -> &str {
    match self {
      CollabGroup::Default(group) => group.workspace_id(),
      CollabGroup::Database(group) => group.workspace_id(),
    }
  }
}

pub trait SubscriptionSink:
  Sink<CollabMessage, Error = RealtimeError> + Send + Sync + Unpin
{
}
impl<T> SubscriptionSink for T where
  T: Sink<CollabMessage, Error = RealtimeError> + Send + Sync + Unpin
{
}

pub trait SubscriptionStream: Stream<Item = MessageByObjectId> + Send + Sync + Unpin {}
impl<T> SubscriptionStream for T where T: Stream<Item = MessageByObjectId> + Send + Sync + Unpin {}
