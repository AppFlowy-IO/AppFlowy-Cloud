use database::collab::CollabStorage;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::registry::Registry;
use std::sync::atomic::{AtomicI64, AtomicU64};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::interval;

#[derive(Clone)]
pub struct CollabRealtimeMetrics {
  connected_users: Gauge,
  encode_collab_mem_hit_rate: Gauge<f64, AtomicU64>,
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
      encode_collab_mem_hit_rate: Gauge::default(),
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
      "mem_hit_rate",
      "memory hit rate",
      metrics.encode_collab_mem_hit_rate.clone(),
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

  pub fn record_encode_collab_mem_hit_rate(&self, rate: f64) {
    self.encode_collab_mem_hit_rate.set(rate);
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
      metrics.record_encode_collab_mem_hit_rate(storage.encode_collab_mem_hit_rate());
    }
  });
}
