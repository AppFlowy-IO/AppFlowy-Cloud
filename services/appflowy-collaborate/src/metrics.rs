use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

#[derive(Clone)]
pub struct CollabRealtimeMetrics {
  pub(crate) connected_users: Gauge,
  pub(crate) opening_collab_count: Gauge,
  pub(crate) num_of_editing_users: Gauge,
  /// The number of apply update
  pub(crate) apply_update_count: Gauge,
  /// The number of apply update failed
  pub(crate) apply_update_failed_count: Gauge,
  pub(crate) acquire_collab_lock_count: Gauge,
  pub(crate) acquire_collab_lock_fail_count: Gauge,
  /// How long it takes to apply update in milliseconds.
  pub(crate) apply_update_time: Histogram,
  /// How big the update is in bytes.
  pub(crate) apply_update_size: Histogram,
}

impl CollabRealtimeMetrics {
  fn new() -> Self {
    Self {
      connected_users: Gauge::default(),
      opening_collab_count: Gauge::default(),
      num_of_editing_users: Gauge::default(),
      apply_update_count: Default::default(),
      apply_update_failed_count: Default::default(),
      acquire_collab_lock_count: Default::default(),
      acquire_collab_lock_fail_count: Default::default(),

      // when it comes to histograms we organize them by buckets or specific sizes - since our
      // prometheus client doesn't support Summary type, we use Histogram type instead

      // time spent on apply_update in milliseconds: 1ms, 5ms, 15ms, 30ms, 100ms, 200ms, 500ms, 1s
      apply_update_time: Histogram::new(
        [1.0, 5.0, 15.0, 30.0, 100.0, 200.0, 500.0, 1000.0].into_iter(),
      ),
      // update size in bytes: 128B, 512B, 1KB, 64KB, 512KB, 1MB, 5MB, 10MB
      apply_update_size: Histogram::new(
        [
          128.0, 512.0, 1024.0, 65536.0, 524288.0, 1048576.0, 5242880.0, 10485760.0,
        ]
        .into_iter(),
      ),
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
    realtime_registry.register(
      "apply_update_time",
      "time spent on applying collab updates in milliseconds",
      metrics.apply_update_time.clone(),
    );
    realtime_registry.register(
      "apply_update_size",
      "size of updates applied to collab in bytes",
      metrics.apply_update_size.clone(),
    );

    metrics
  }
}

#[derive(Clone)]
pub struct CollabMetrics {
  pub write_snapshot: Counter,
  pub write_snapshot_failures: Counter,
  pub read_snapshot: Counter,
  pub pg_write_collab_count: Counter,
  pub s3_write_collab_count: Counter,
  pub redis_write_collab_count: Counter,
  pub pg_read_collab_count: Counter,
  pub s3_read_collab_count: Counter,
  pub redis_read_collab_count: Counter,
  pub success_queue_collab_count: Counter,
  pg_tx_collab_millis: Histogram,
}

impl CollabMetrics {
  pub fn register(registry: &mut Registry) -> Self {
    let metrics = Self::default();
    let realtime_registry = registry.sub_registry_with_prefix("collab");
    realtime_registry.register(
      "write_snapshot",
      "snapshot write attempts counter",
      metrics.write_snapshot.clone(),
    );
    realtime_registry.register(
      "write_snapshot_failures",
      "counter for failed attempts to write a snapshot",
      metrics.write_snapshot_failures.clone(),
    );
    realtime_registry.register(
      "read_snapshot",
      "snapshot read counter",
      metrics.read_snapshot.clone(),
    );
    realtime_registry.register(
      "pg_write_collab_count",
      "success write collab to Postgres",
      metrics.pg_write_collab_count.clone(),
    );
    realtime_registry.register(
      "s3_write_collab_count",
      "success write collab to S3",
      metrics.s3_write_collab_count.clone(),
    );
    realtime_registry.register(
      "redis_write_collab_count",
      "success write collab to Redis",
      metrics.redis_write_collab_count.clone(),
    );
    realtime_registry.register(
      "pg_read_collab_count",
      "success read collabs from Postgres",
      metrics.pg_read_collab_count.clone(),
    );
    realtime_registry.register(
      "s3_read_collab_count",
      "success read collabs from S3",
      metrics.s3_read_collab_count.clone(),
    );
    realtime_registry.register(
      "redis_read_collab_count",
      "success read collabs from Redis",
      metrics.redis_read_collab_count.clone(),
    );
    realtime_registry.register(
      "success_queue_collab_count",
      "success queue collab",
      metrics.success_queue_collab_count.clone(),
    );
    realtime_registry.register(
      "pg_tx_collab_millis",
      "total time (in milliseconds) spend in transaction writing collab to postgres",
      metrics.pg_tx_collab_millis.clone(),
    );

    metrics
  }

  pub fn observe_pg_tx(&self, duration: std::time::Duration) {
    self
      .pg_tx_collab_millis
      .observe(duration.as_millis() as f64);
  }
}

impl Default for CollabMetrics {
  fn default() -> Self {
    CollabMetrics {
      write_snapshot: Default::default(),
      write_snapshot_failures: Default::default(),
      read_snapshot: Default::default(),
      pg_write_collab_count: Default::default(),
      s3_write_collab_count: Default::default(),
      redis_write_collab_count: Default::default(),
      pg_read_collab_count: Default::default(),
      s3_read_collab_count: Default::default(),
      redis_read_collab_count: Default::default(),
      success_queue_collab_count: Default::default(),
      pg_tx_collab_millis: Histogram::new(
        [
          100.0, 300.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 30000.0, 60000.0,
        ]
        .into_iter(),
      ),
    }
  }
}
