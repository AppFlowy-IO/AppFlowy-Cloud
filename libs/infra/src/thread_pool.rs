use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rayon::{ThreadPool, ThreadPoolBuilder};
use thiserror::Error;

/// A thread pool that does not abort on panics.
///
/// This custom thread pool wraps Rayon’s `ThreadPool` and ensures that the thread pool
/// can recover from panics gracefully. It detects any panics in worker threads and
/// prevents the entire application from aborting.
#[derive(Debug)]
pub struct ThreadPoolNoAbort {
  /// Internal Rayon thread pool.
  thread_pool: ThreadPool,
  /// Atomic flag to detect if a panic occurred in the thread pool.
  catched_panic: Arc<AtomicBool>,
}

impl ThreadPoolNoAbort {
  /// Executes a closure within the thread pool.
  ///
  /// This method runs the provided closure (`op`) inside the thread pool. If a panic
  /// occurs during the execution, it is detected and returned as an error.
  ///
  /// # Arguments
  /// * `op` - A closure that will be executed within the thread pool.
  ///
  /// # Returns
  /// * `Ok(R)` - The result of the closure if execution was successful.
  /// * `Err(PanicCatched)` - An error indicating that a panic occurred during execution.
  ///
  pub fn install<OP, R>(&self, op: OP) -> Result<R, CatchedPanic>
  where
    OP: FnOnce() -> R + Send,
    R: Send,
  {
    let output = self.thread_pool.install(op);
    // Reset the panic flag and return an error if a panic was detected.
    if self.catched_panic.swap(false, Ordering::SeqCst) {
      Err(CatchedPanic)
    } else {
      Ok(output)
    }
  }

  /// Returns the current number of threads in the thread pool.
  ///
  /// # Returns
  /// The number of threads being used by the thread pool.
  pub fn current_num_threads(&self) -> usize {
    self.thread_pool.current_num_threads()
  }
}

/// Error indicating that a panic occurred during thread pool execution.
///
/// This error is returned when a closure executed in the thread pool panics.
#[derive(Error, Debug)]
#[error("A panic occurred happened in the thread pool. Check the logs for more information")]
pub struct CatchedPanic;

/// A builder for creating a `ThreadPoolNoAbort` instance.
///
/// This builder wraps Rayon’s `ThreadPoolBuilder` and customizes the panic handling behavior.
#[derive(Default)]
pub struct ThreadPoolNoAbortBuilder(ThreadPoolBuilder);

impl ThreadPoolNoAbortBuilder {
  pub fn new() -> ThreadPoolNoAbortBuilder {
    ThreadPoolNoAbortBuilder::default()
  }

  /// Sets a custom naming function for threads in the pool.
  ///
  /// # Arguments
  /// * `closure` - A function that takes a thread index and returns a thread name.
  ///
  pub fn thread_name<F>(mut self, closure: F) -> Self
  where
    F: FnMut(usize) -> String + 'static,
  {
    self.0 = self.0.thread_name(closure);
    self
  }

  /// Sets the number of threads for the thread pool.
  ///
  /// # Arguments
  /// * `num_threads` - The number of threads to create in the thread pool.
  pub fn num_threads(mut self, num_threads: usize) -> ThreadPoolNoAbortBuilder {
    self.0 = self.0.num_threads(num_threads);
    self
  }

  /// Builds the `ThreadPoolNoAbort` instance.
  ///
  /// This method creates a `ThreadPoolNoAbort` with the specified configurations,
  /// including custom panic handling behavior.
  ///
  /// # Returns
  /// * `Ok(ThreadPoolNoAbort)` - The constructed thread pool.
  /// * `Err(ThreadPoolBuildError)` - If the thread pool failed to build.
  ///
  pub fn build(mut self) -> Result<ThreadPoolNoAbort, rayon::ThreadPoolBuildError> {
    let catched_panic = Arc::new(AtomicBool::new(false));
    self.0 = self.0.panic_handler({
      let catched_panic = catched_panic.clone();
      move |_result| catched_panic.store(true, Ordering::SeqCst)
    });
    Ok(ThreadPoolNoAbort {
      thread_pool: self.0.build()?,
      catched_panic,
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::sync::atomic::{AtomicUsize, Ordering};

  #[test]
  fn test_install_closure_success() {
    // Create a thread pool with 4 threads.
    let pool = ThreadPoolNoAbortBuilder::new()
      .num_threads(4)
      .build()
      .expect("Failed to build thread pool");

    // Run a closure that executes successfully.
    let result = pool.install(|| 42);

    // Ensure the result is correct.
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 42);
  }

  #[test]
  fn test_multiple_threads_execution() {
    // Create a thread pool with multiple threads.
    let pool = ThreadPoolNoAbortBuilder::new()
      .num_threads(8)
      .build()
      .expect("Failed to build thread pool");

    // Shared atomic counter to verify parallel execution.
    let counter = Arc::new(AtomicUsize::new(0));

    let handles: Vec<_> = (0..100)
      .map(|_| {
        let counter_clone = counter.clone();
        pool.install(move || {
          counter_clone.fetch_add(1, Ordering::SeqCst);
        })
      })
      .collect();

    // Ensure all tasks completed successfully.
    for handle in handles {
      assert!(handle.is_ok());
    }

    // Verify that the counter equals the number of tasks executed.
    assert_eq!(counter.load(Ordering::SeqCst), 100);
  }
}
