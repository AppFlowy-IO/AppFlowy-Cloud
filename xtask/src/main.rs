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
/// Note: test start with 'stress_test' will be run as stress tests
#[tokio::main]
async fn main() -> Result<()> {
  let is_stress_test = std::env::args().any(|arg| arg == "--stress-test");
  let disable_log = std::env::args().any(|arg| arg == "--disable-log");
  let target_dir = "./target";
  std::env::set_var("CARGO_TARGET_DIR", target_dir);

  let appflowy_cloud_bin_name = "appflowy_cloud";
  let worker_bin_name = "appflowy_worker";

  // Step 1: Kill existing processes
  kill_existing_process(appflowy_cloud_bin_name).await?;
  kill_existing_process(worker_bin_name).await?;

  // Step 2: Start servers sequentially
  println!("Starting {} server...", appflowy_cloud_bin_name);
  let mut appflowy_cloud_cmd = spawn_server(
    "cargo",
    &["run", "--features", "history, use_actix_cors"],
    appflowy_cloud_bin_name,
    disable_log,
    None,
    // Some(vec![("RUSTFLAGS", "--cfg tokio_unstable")]),
  )?;
  wait_for_readiness(appflowy_cloud_bin_name).await?;

  println!("Starting {} server...", worker_bin_name);
  let mut appflowy_worker_cmd = spawn_server(
    "cargo",
    &[
      "run",
      "--manifest-path",
      "./services/appflowy-worker/Cargo.toml",
    ],
    worker_bin_name,
    disable_log,
    None,
  )?;
  wait_for_readiness(worker_bin_name).await?;

  println!("All servers are up and running.");

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

  // Step 4: Monitor all processes
  select! {
      status = appflowy_cloud_cmd.wait() => {
          handle_process_exit(status?, worker_bin_name)?;
      },
      status = appflowy_worker_cmd.wait() => {
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

  Ok(())
}

fn spawn_server(
  command: &str,
  args: &[&str],
  name: &str,
  suppress_output: bool,
  env_vars: Option<Vec<(&str, &str)>>,
) -> Result<tokio::process::Child> {
  println!(
    "Spawning {} process..., log enabled:{}",
    name, suppress_output
  );
  let mut cmd = Command::new(command);
  cmd.args(args);

  // Set environment variables if provided
  if let Some(vars) = env_vars {
    for (key, value) in vars {
      cmd.env(key, value);
    }
  }

  if suppress_output {
    cmd.stdout(Stdio::null()).stderr(Stdio::null());
  }

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
