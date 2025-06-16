use anyhow::{anyhow, Context, Result};
use std::process::Stdio;
use tokio::process::Command;
use tokio::select;
use tokio::time::{sleep, Duration};

/// Run servers:
/// cargo run --package xtask
///
/// Run servers and stress tests:
/// cargo run --package xtask -- --stress-test
///
/// Run without appflowy-worker:
/// cargo run --package xtask -- --no-worker
///
/// Run with optimizations (recommended for development):
/// cargo run --package xtask -- --ci
///
/// Run with full optimizations (production):
/// cargo run --package xtask -- --release
///
/// Run with profiling enabled:
/// cargo run --package xtask -- --profiling
///
/// Combine flags:
/// cargo run --package xtask -- --ci --stress-test --no-worker
///
/// Note: test start with 'stress_test' will be run as stress tests
#[tokio::main]
async fn main() -> Result<()> {
  let is_stress_test = std::env::args().any(|arg| arg == "--stress-test");
  let no_worker = std::env::args().any(|arg| arg == "--no-worker");
  let target_dir = "./target";
  std::env::set_var("CARGO_TARGET_DIR", target_dir);

  // Optimize Cargo for faster builds
  std::env::set_var("CARGO_INCREMENTAL", "1");

  // Only use sccache if it's available
  if Command::new("sccache")
    .arg("--version")
    .output()
    .await
    .is_ok()
  {
    std::env::set_var("RUSTC_WRAPPER", "sccache");
    println!("Using sccache for faster compilation");
  }

  // Add profile flags for optimized performance
  let profile_flags = if std::env::args().any(|arg| arg == "--release") {
    vec!["--profile", "release"] // Full optimization with LTO
  } else if std::env::args().any(|arg| arg == "--ci") {
    vec!["--profile", "ci"] // Faster compile, still optimized
  } else if std::env::args().any(|arg| arg == "--profiling") {
    vec!["--profile", "profiling"] // Release + debug info
  } else {
    vec![] // Default dev profile
  };

  let appflowy_cloud_bin_name = "appflowy_cloud";
  let worker_bin_name = "appflowy_worker";

  // Show which profile is being used
  let profile_name = if profile_flags.contains(&"release") {
    "release (full optimization + LTO)"
  } else if profile_flags.contains(&"ci") {
    "ci (fast compile + optimized)"
  } else if profile_flags.contains(&"profiling") {
    "profiling (release + debug info)"
  } else {
    "dev (fastest compile)"
  };
  println!("Using profile: {}", profile_name);

  if no_worker {
    println!("Worker disabled - running without appflowy-worker");
  }

  // Step 1: Kill existing processes
  let kill_cloud_result = kill_existing_process(appflowy_cloud_bin_name);
  let kill_worker_result = if no_worker {
    // Still kill existing worker processes if they exist, even if we won't start a new one
    kill_existing_process(worker_bin_name)
  } else {
    kill_existing_process(worker_bin_name)
  };

  let (kill_result1, kill_result2) = tokio::join!(kill_cloud_result, kill_worker_result);
  kill_result1?;
  kill_result2?;

  // Step 2: Start servers
  if no_worker {
    println!("Starting appflowy-cloud only...");
  } else {
    println!("Starting servers in parallel...");
  }

  // Prepare args with profile flags if specified
  let mut cloud_args = vec!["run"];
  cloud_args.extend(&profile_flags);
  cloud_args.extend(&["--features", "history, use_actix_cors"]);

  let appflowy_cloud_handle = tokio::spawn(async move {
    let cmd = spawn_server("cargo", &cloud_args, appflowy_cloud_bin_name)?;
    wait_for_readiness(appflowy_cloud_bin_name).await?;
    Ok::<_, anyhow::Error>(cmd)
  });

  let appflowy_worker_handle = if no_worker {
    None
  } else {
    let mut worker_args = vec!["run"];
    worker_args.extend(&profile_flags);
    worker_args.extend(&["--manifest-path", "./services/appflowy-worker/Cargo.toml"]);

    Some(tokio::spawn(async move {
      let cmd = spawn_server("cargo", &worker_args, worker_bin_name)?;
      wait_for_readiness(worker_bin_name).await?;
      Ok::<_, anyhow::Error>(cmd)
    }))
  };

  // Wait for servers to be ready
  let mut appflowy_cloud_cmd = appflowy_cloud_handle.await??;
  let appflowy_worker_cmd = if let Some(handle) = appflowy_worker_handle {
    Some(handle.await??)
  } else {
    None
  };

  if no_worker {
    println!("AppFlowy Cloud is up and running (worker disabled).");
  } else {
    println!("All servers are up and running.");
  }

  // Step 3: Run stress tests if flag is set
  let stress_test_cmd = if is_stress_test {
    println!("Running stress tests (tests starting with 'stress_test')...");
    Some(
      Command::new("cargo")
        .args(["test", "stress_test", "--", "--nocapture"])
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .context("Failed to start stress test process")?,
    )
  } else {
    None
  };

  // Step 4: Monitor processes
  match appflowy_worker_cmd {
    Some(mut worker_cmd) => {
      // Monitor both cloud and worker
      select! {
          status = appflowy_cloud_cmd.wait() => {
              handle_process_exit(status?, appflowy_cloud_bin_name)?;
          },
          status = worker_cmd.wait() => {
              handle_process_exit(status?, worker_bin_name)?;
          },
          status = async {
              if let Some(mut stress_cmd) = stress_test_cmd {
                  stress_cmd.wait().await
              } else {
                  futures::future::pending().await
              }
          } => {
              if is_stress_test {
                  handle_process_exit(status?, "cargo test stress_test")?;
              }
          },
      }
    },
    None => {
      // Monitor only cloud (no worker)
      select! {
          status = appflowy_cloud_cmd.wait() => {
              handle_process_exit(status?, appflowy_cloud_bin_name)?;
          },
          status = async {
              if let Some(mut stress_cmd) = stress_test_cmd {
                  stress_cmd.wait().await
              } else {
                  futures::future::pending().await
              }
          } => {
              if is_stress_test {
                  handle_process_exit(status?, "cargo test stress_test")?;
              }
          },
      }
    },
  }

  Ok(())
}

fn spawn_server(command: &str, args: &[&str], name: &str) -> Result<tokio::process::Child> {
  println!("Spawning {} process...", name,);
  let mut cmd = Command::new(command);
  cmd.args(args);
  cmd
    .spawn()
    .context(format!("Failed to start {} process", name))
}

async fn kill_existing_process(process_identifier: &str) -> Result<()> {
  let _ = Command::new("pkill")
    .arg("-f")
    .arg(process_identifier)
    .output()
    .await
    .context("Failed to kill existing processes")?;
  println!("Killed existing instances of {}", process_identifier);
  Ok(())
}

fn handle_process_exit(status: std::process::ExitStatus, process_name: &str) -> Result<()> {
  if status.success() {
    println!("handle_process_exit: {} exited normally.", process_name);
    Ok(())
  } else {
    Err(anyhow!(
      "handle_process_exit: {} process failed: {}",
      process_name,
      status
    ))
  }
}

async fn wait_for_readiness(process_name: &str) -> Result<()> {
  println!("Waiting for {} to be ready...", process_name);
  sleep(Duration::from_secs(3)).await;
  println!("{} is ready.", process_name);
  Ok(())
}
