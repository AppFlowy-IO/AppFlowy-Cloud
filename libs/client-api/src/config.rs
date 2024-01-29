use tracing::warn;

#[derive(Clone)]
pub struct ClientConfiguration {
  /// Lower Levels (0-4): Faster compression and decompression speeds but lower compression ratios. Suitable for scenarios where speed is more critical than reducing data size.
  /// Medium Levels (5-9): A balance between compression ratio and speed. These levels are generally good for a mix of performance and efficiency.
  /// Higher Levels (10-11): The highest compression ratios, but significantly slower and more resource-intensive. These are typically used in scenarios where reducing data size is paramount and resource usage is a secondary concern, such as for static content compression in web servers.
  pub(crate) compression_quality: u32,
  /// A larger buffer size means more data is compressed in a single operation, which can lead to better compression ratios
  /// since Brotli has more data to analyze for patterns and repetitions.
  pub(crate) compression_buffer_size: usize,
}

impl ClientConfiguration {
  pub fn with_compression_buffer_size(mut self, compression_buffer_size: usize) -> Self {
    self.compression_buffer_size = compression_buffer_size;
    self
  }

  pub fn with_compression_quality(mut self, compression_quality: u32) -> Self {
    self.compression_quality = if compression_quality > 11 {
      warn!("compression_quality is larger than 11, set it to 11");
      11
    } else {
      compression_quality
    };
    self
  }
}

impl Default for ClientConfiguration {
  fn default() -> Self {
    Self {
      compression_quality: 8,
      compression_buffer_size: 10240,
    }
  }
}
