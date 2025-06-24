use chrono::Utc;
use prometheus_client::metrics::counter::Counter;
use prometheus_client::metrics::gauge::Gauge;
use prometheus_client::metrics::histogram::Histogram;
use prometheus_client::registry::Registry;

#[derive(Clone)]
pub struct CollabRealtimeMetrics {
  pub(crate) connected_users: Gauge,
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
  /// How long does it take since collab update is send to a stream to be read from it.
  pub(crate) collab_stream_latency: Histogram,
}

impl CollabRealtimeMetrics {
  fn new() -> Self {
    Self {
      connected_users: Gauge::default(),
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
      // collab update xadd-to-xread latency: 5ms, 50ms, 100ms, 500ms, 1s, 5s, 10s, 30s, 60s
      collab_stream_latency: Histogram::new(
        [
          5.0, 50.0, 100.0, 500.0, 1000.0, 5000.0, 10000.0, 30000.0, 60000.0,
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
    realtime_registry.register(
      "collab_stream_latency",
      "latency since collab update is send to a stream to be read from it",
      metrics.collab_stream_latency.clone(),
    );
    metrics
  }

  pub fn observe_collab_stream_latency(&self, message_id_timestamp: u64) {
    let now = Utc::now().timestamp_millis() as u64;
    if now > message_id_timestamp {
      self
        .collab_stream_latency
        .observe((now - message_id_timestamp) as f64);
    }
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
  /// Duration of workspace snapshot operations in milliseconds
  pub snapshot_duration: Histogram,
  /// Number of updates processed in each workspace snapshot
  pub snapshot_updates_count: Histogram,
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
    realtime_registry.register(
      "snapshot_duration",
      "duration of workspace snapshot operations in milliseconds",
      metrics.snapshot_duration.clone(),
    );
    realtime_registry.register(
      "snapshot_updates_count",
      "number of updates processed in each workspace snapshot",
      metrics.snapshot_updates_count.clone(),
    );

    metrics
  }

  pub fn observe_pg_tx(&self, duration: std::time::Duration) {
    self
      .pg_tx_collab_millis
      .observe(duration.as_millis() as f64);
  }

  pub fn observe_snapshot_duration(&self, duration: std::time::Duration) {
    self.snapshot_duration.observe(duration.as_millis() as f64);
  }

  pub fn observe_snapshot_updates_count(&self, count: usize) {
    self.snapshot_updates_count.observe(count as f64);
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
      // Snapshot duration buckets: 100ms, 500ms, 1s, 5s, 10s, 30s, 60s, 120s, 300s
      snapshot_duration: Histogram::new(
        [
          100.0, 500.0, 1000.0, 5000.0, 10000.0, 30000.0, 60000.0, 120000.0, 300000.0,
        ]
        .into_iter(),
      ),
      // Snapshot updates count buckets: 1, 10, 50, 100, 500, 1000, 5000, 10000, 50000
      snapshot_updates_count: Histogram::new(
        [
          1.0, 10.0, 50.0, 100.0, 500.0, 1000.0, 5000.0, 10000.0, 50000.0,
        ]
        .into_iter(),
      ),
    }
  }
}
