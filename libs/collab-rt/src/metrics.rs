use crate::collaborate::all_group::AllGroup;
use crate::collaborate::group_cmd::GroupCommandSender;
use crate::{CollabClientStream, RealtimeAccessControl};
use collab_rt_entity::user::RealtimeUser;
use dashmap::DashMap;
use database::collab::CollabStorage;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Weak};
use std::time::Duration;
use tokio::time::interval;

#[derive(Clone)]
pub struct CollabRealtimeMetrics {
  connected_users: Gauge,
  encode_collab_mem_hit_rate: Gauge<f64, AtomicU64>,
  opening_collab_count: Gauge,
}

impl CollabRealtimeMetrics {
  fn init() -> Self {
    Self {
      connected_users: Gauge::default(),
      encode_collab_mem_hit_rate: Gauge::default(),
      opening_collab_count: Gauge::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let realtime_registry = registry.sub_registry_with_prefix("realtime");
    realtime_registry.register(
      "connected_users",
      "number of connected users",
      metrics.connected_users.clone(),
    );
    realtime_registry.register(
      "mem_hit_rate",
      "memory hit rate",
      metrics.encode_collab_mem_hit_rate.clone(),
    );
    realtime_registry.register(
      "opening_collab_count",
      "number of opening collabs",
      metrics.opening_collab_count.clone(),
    );

    metrics
  }

  pub fn record_connected_users(&self, num: usize) {
    self.connected_users.set(num as i64);
  }

  pub fn record_encode_collab_mem_hit_rate(&self, rate: f64) {
    self.encode_collab_mem_hit_rate.set(rate);
  }

  pub fn record_opening_collab_count(&self, count: usize) {
    self.opening_collab_count.set(count as i64);
  }
}

pub(crate) fn spawn_metrics<S, U, AC>(
  group_sender_by_object_id: &Arc<DashMap<String, GroupCommandSender<U>>>,
  weak_groups: Weak<AllGroup<S, U, AC>>,
  metrics: &Arc<CollabRealtimeMetrics>,
  client_stream_by_user: &Arc<DashMap<U, CollabClientStream>>,
  storage: &Arc<S>,
) where
  S: CollabStorage,
  U: RealtimeUser,
  AC: RealtimeAccessControl,
{
  let metrics = metrics.clone();
  let client_stream_by_user = client_stream_by_user.clone();
  let storage = storage.clone();

  let cloned_group_sender_by_object_id = group_sender_by_object_id.clone();
  tokio::task::spawn_local(async move {
    let mut interval = if cfg!(debug_assertions) {
      interval(Duration::from_secs(10))
    } else {
      interval(Duration::from_secs(60))
    };
    loop {
      interval.tick().await;
      if let Some(groups) = weak_groups.upgrade() {
        let inactive_group_ids = groups.tick().await;
        for id in inactive_group_ids {
          cloned_group_sender_by_object_id.remove(&id);
        }

        metrics.record_opening_collab_count(groups.number_of_groups().await);
        metrics.record_connected_users(client_stream_by_user.len());
        metrics.record_encode_collab_mem_hit_rate(storage.encode_collab_mem_hit_rate());
      } else {
        break;
      }
    }
  });
}
