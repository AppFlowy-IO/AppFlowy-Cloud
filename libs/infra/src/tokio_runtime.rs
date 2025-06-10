use std::io;
use std::sync::LazyLock;

use tokio::runtime;
use tokio::runtime::Runtime;

pub static AF_SINGLE_THREAD_RUNTIME: LazyLock<Runtime> =
  LazyLock::new(|| default_tokio_runtime().unwrap());

fn default_tokio_runtime() -> io::Result<Runtime> {
  runtime::Builder::new_multi_thread()
    .worker_threads(1)
    .thread_name("af-runtime")
    .enable_io()
    .enable_time()
    .build()
}
