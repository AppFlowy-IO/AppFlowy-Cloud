use std::sync::Arc;
use std::time::Duration;

use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;
use tokio::time::interval;

use database::collab::CollabStorage;

#[derive(Clone)]
pub struct CollabRealtimeMetrics {
  pub(crate) connected_users: Gauge,
  pub(crate) total_success_get_encode_collab_from_redis: Gauge,
  pub(crate) total_attempt_get_encode_collab_from_redis: Gauge,
  pub(crate) opening_collab_count: Gauge,
  pub(crate) num_of_editing_users: Gauge,
  /// Number of times a compact state collab load has been done.
  pub(crate) load_collab_count: Gauge,
  /// Number of times a full state collab (with history) load has been done.
  pub(crate) load_full_collab_count: Gauge,
  /// The number of apply update
  pub(crate) apply_update_count: Gauge,
  /// The number of apply update failed
  pub(crate) apply_update_failed_count: Gauge,
  /// How long it takes to load a collab (from snapshot and updates combined).
  pub(crate) load_collab_time: Histogram,
  /// How big is the collab (no history, after applying all updates).
  pub(crate) collab_size: Histogram,
  /// How big is the collab (with history, after applying all updates).
  pub(crate) full_collab_size: Histogram,
}

impl CollabRealtimeMetrics {
  fn new() -> Self {
    Self {
      connected_users: Gauge::default(),
      total_success_get_encode_collab_from_redis: Gauge::default(),
      total_attempt_get_encode_collab_from_redis: Gauge::default(),
      opening_collab_count: Gauge::default(),
      num_of_editing_users: Gauge::default(),
      apply_update_count: Default::default(),
      apply_update_failed_count: Default::default(),

      // when it comes to histograms we organize them by buckets or specific sizes - since our
      // prometheus client doesn't support Summary type, we use Histogram type instead

      // time spent on loading collab in milliseconds: 1ms, 5ms, 15ms, 30ms, 100ms, 200ms, 500ms, 1s
      load_collab_time: Histogram::new(
        [1.0, 5.0, 15.0, 30.0, 100.0, 200.0, 500.0, 1000.0].into_iter(),
      ),
      // collab size in bytes: 128B, 512B, 1KB, 64KB, 512KB, 1MB, 5MB, 10MB
      collab_size: Histogram::new(
        [
          128.0, 512.0, 1024.0, 65536.0, 524288.0, 1048576.0, 5242880.0, 10485760.0,
        ]
        .into_iter(),
      ),
      // collab size in bytes: 128B, 512B, 1KB, 64KB, 512KB, 1MB, 5MB, 10MB
      full_collab_size: Histogram::new(
        [
          128.0, 512.0, 1024.0, 65536.0, 524288.0, 1048576.0, 5242880.0, 10485760.0,
        ]
        .into_iter(),
      ),
      load_collab_count: Default::default(),
      load_full_collab_count: Default::default(),
    }
  }

  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::new();
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
      "load_collab_time",
      "time spent on loading collab in milliseconds",
      metrics.load_collab_time.clone(),
    );
    realtime_registry.register(
      "collab_size",
      "size of compact collab in bytes",
      metrics.collab_size.clone(),
    );
    realtime_registry.register(
      "full_collab_size",
      "size of full collab in bytes",
      metrics.full_collab_size.clone(),
    );
    realtime_registry.register(
      "load_collab_count",
      "number of collab loads (no history)",
      metrics.load_collab_count.clone(),
    );
    realtime_registry.register(
      "load_full_collab_count",
      "number of collab loads (with history)",
      metrics.load_full_collab_count.clone(),
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
