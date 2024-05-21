use database::collab::CollabStorage;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;
use std::sync::atomic::AtomicI64;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

#[derive(Clone)]
pub struct CollabRealtimeMetrics {
  connected_users: Gauge,
  total_success_get_encode_collab_from_redis: Gauge,
  total_attempt_get_encode_collab_from_redis: Gauge,
  opening_collab_count: Gauge,
  /// The number of apply update
  apply_update_count: Gauge,
  /// The number of apply update failed
  apply_update_failed_count: Gauge,
  acquire_collab_lock_count: Gauge,
  acquire_collab_lock_fail_count: Gauge,
}

impl CollabRealtimeMetrics {
  fn init() -> Self {
    Self {
      connected_users: Gauge::default(),
      total_success_get_encode_collab_from_redis: Gauge::default(),
      total_attempt_get_encode_collab_from_redis: Gauge::default(),
      opening_collab_count: Gauge::default(),
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

#[derive(Clone, Default)]
pub(crate) struct CollabMetricsCalculate {
  pub(crate) connected_users: Arc<AtomicI64>,
  pub(crate) acquire_collab_lock_count: Arc<AtomicI64>,
  pub(crate) acquire_collab_lock_fail_count: Arc<AtomicI64>,
  pub(crate) apply_update_count: Arc<AtomicI64>,
  pub(crate) apply_update_failed_count: Arc<AtomicI64>,
  pub(crate) num_of_active_collab: Arc<AtomicI64>,
}

pub(crate) fn spawn_metrics<S>(
  metrics: &Arc<CollabRealtimeMetrics>,
  metrics_calculation: &CollabMetricsCalculate,
  storage: &Arc<S>,
) where
  S: CollabStorage,
{
  let metrics = metrics.clone();
  let metrics_calculation = metrics_calculation.clone();
  let storage = storage.clone();
  tokio::task::spawn_local(async move {
    let mut interval = interval(Duration::from_secs(120));
    loop {
      interval.tick().await;

      // active collab
      metrics.opening_collab_count.set(
        metrics_calculation
          .num_of_active_collab
          .load(std::sync::atomic::Ordering::Relaxed),
      );

      // connect user
      metrics.connected_users.set(
        metrics_calculation
          .connected_users
          .load(std::sync::atomic::Ordering::Relaxed),
      );

      // lock
      metrics.acquire_collab_lock_count.set(
        metrics_calculation
          .acquire_collab_lock_count
          .load(std::sync::atomic::Ordering::Relaxed),
      );
      metrics.acquire_collab_lock_fail_count.set(
        metrics_calculation
          .acquire_collab_lock_fail_count
          .load(std::sync::atomic::Ordering::Relaxed),
      );

      // update count
      metrics.apply_update_count.set(
        metrics_calculation
          .apply_update_count
          .load(std::sync::atomic::Ordering::Relaxed),
      );
      metrics.apply_update_failed_count.set(
        metrics_calculation
          .apply_update_failed_count
          .load(std::sync::atomic::Ordering::Relaxed),
      );

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
