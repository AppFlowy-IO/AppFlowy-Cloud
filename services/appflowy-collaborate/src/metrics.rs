use std::sync::Arc;
use std::time::Duration;

use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;
use tokio::time::interval;

use database::collab::CollabStorage;

#[derive(Clone, Default)]
pub struct CollabRealtimeMetrics {
  pub(crate) connected_users: Gauge,
  pub(crate) total_success_get_encode_collab_from_redis: Gauge,
  pub(crate) total_attempt_get_encode_collab_from_redis: Gauge,
  pub(crate) opening_collab_count: Gauge,
  pub(crate) num_of_editing_users: Gauge,
  /// The number of apply update
  pub(crate) apply_update_count: Gauge,
  /// The number of apply update failed
  pub(crate) apply_update_failed_count: Gauge,
  pub(crate) acquire_collab_lock_count: Gauge,
  pub(crate) acquire_collab_lock_fail_count: Gauge,
}

impl CollabRealtimeMetrics {
  fn init() -> Self {
    Self {
      connected_users: Gauge::default(),
      total_success_get_encode_collab_from_redis: Gauge::default(),
      total_attempt_get_encode_collab_from_redis: Gauge::default(),
      opening_collab_count: Gauge::default(),
      num_of_editing_users: Gauge::default(),
      apply_update_count: Default::default(),
      apply_update_failed_count: Default::default(),
      acquire_collab_lock_count: Default::default(),
      acquire_collab_lock_fail_count: Default::default(),
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
      "total_success_get_encode_collab_from_redis",
      "total success get encode collab from redis",
      metrics.total_success_get_encode_collab_from_redis.clone(),
    );
    realtime_registry.register(
      "total_attempt_get_encode_collab_from_redis",
      "total attempt get encode collab from redis",
      metrics.total_attempt_get_encode_collab_from_redis.clone(),
    );
    realtime_registry.register(
      "opening_collab_count",
      "number of opening collabs",
      metrics.opening_collab_count.clone(),
    );
    realtime_registry.register(
      "editing_users_count",
      "number of editing users",
      metrics.num_of_editing_users.clone(),
    );
    realtime_registry.register(
      "apply_update_count",
      "number of apply update",
      metrics.apply_update_count.clone(),
    );
    realtime_registry.register(
      "apply_update_failed_count",
      "number of apply update failed",
      metrics.apply_update_failed_count.clone(),
    );

    realtime_registry.register(
      "acquire_collab_lock_count",
      "number of acquire collab lock",
      metrics.acquire_collab_lock_count.clone(),
    );
    realtime_registry.register(
      "acquire_collab_lock_fail_count",
      "number of acquire collab lock failed",
      metrics.acquire_collab_lock_fail_count.clone(),
    );

    metrics
  }
}

pub(crate) fn spawn_metrics<S>(metrics: Arc<CollabRealtimeMetrics>, storage: Arc<S>)
where
  S: CollabStorage,
{
  tokio::task::spawn_local(async move {
    let mut interval = interval(Duration::from_secs(120));
    loop {
      interval.tick().await;

      // cache hit rate
      let (total, success) = storage.encode_collab_redis_query_state();
      metrics
        .total_attempt_get_encode_collab_from_redis
        .set(total as i64);
      metrics
        .total_success_get_encode_collab_from_redis
        .set(success as i64);
    }
  });
}

#[derive(Clone)]
pub struct CollabMetrics {
  success_write_snapshot_count: Gauge,
  total_write_snapshot_count: Gauge,
  success_write_collab_count: Gauge,
  total_write_collab_count: Gauge,
  total_queue_collab_count: Gauge,
  success_queue_collab_count: Gauge,
}

impl CollabMetrics {
  fn init() -> Self {
    Self {
      success_write_snapshot_count: Gauge::default(),
      total_write_snapshot_count: Default::default(),
      success_write_collab_count: Default::default(),
      total_write_collab_count: Default::default(),
      total_queue_collab_count: Default::default(),
      success_queue_collab_count: Default::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::init();
    let realtime_registry = registry.sub_registry_with_prefix("collab");
    realtime_registry.register(
      "success_write_snapshot_count",
      "success write snapshot to db",
      metrics.success_write_snapshot_count.clone(),
    );
    realtime_registry.register(
      "total_attempt_write_snapshot_count",
      "total attempt write snapshot to db",
      metrics.total_write_snapshot_count.clone(),
    );
    realtime_registry.register(
      "success_write_collab_count",
      "success write collab",
      metrics.success_write_collab_count.clone(),
    );
    realtime_registry.register(
      "total_write_collab_count",
      "total write collab",
      metrics.total_write_collab_count.clone(),
    );
    realtime_registry.register(
      "success_queue_collab_count",
      "success queue collab",
      metrics.success_queue_collab_count.clone(),
    );
    realtime_registry.register(
      "total_queue_collab_count",
      "total queue pending collab",
      metrics.total_queue_collab_count.clone(),
    );

    metrics
  }

  pub fn record_write_snapshot(&self, success_attempt: i64, total_attempt: i64) {
    self.success_write_snapshot_count.set(success_attempt);
    self.total_write_snapshot_count.set(total_attempt);
  }

  pub fn record_write_collab(&self, success_attempt: i64, total_attempt: i64) {
    self.success_write_collab_count.set(success_attempt);
    self.total_write_collab_count.set(total_attempt);
  }

  pub fn record_queue_collab(&self, success_attempt: i64, total_attempt: i64) {
    self.success_queue_collab_count.set(success_attempt);
    self.total_queue_collab_count.set(total_attempt);
  }
}
