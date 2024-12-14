use anyhow::{anyhow, Context, Result};
use tokio::process::Command;
use tokio::select;

/// Using 'cargo run --package xtask' to run servers in parallel.
/// 1. AppFlowy Cloud
/// 2. AppFlowy History
/// 3. AppFlowy Indexer
///
/// Before running this command, make sure the other dependencies servers are running. For example,
/// Redis, Postgres, etc.
#[tokio::main]
async fn main() -> Result<()> {
  let appflowy = "appflowy_cloud";
  let worker = "appflowy_worker";
  let target_dir = "./target";
  std::env::set_var("CARGO_TARGET_DIR", target_dir);

  kill_existing_process(appflowy).await?;
  kill_existing_process(worker).await?;

  let enable_runtime_profile = false;
  let mut appflowy_cloud_cmd = Command::new("cargo");

  appflowy_cloud_cmd
    .env("RUSTFLAGS", "--cfg tokio_unstable")
    .args(["run", "--features"]);
  if enable_runtime_profile {
    appflowy_cloud_cmd.args(["history,tokio-runtime-profile"]);
  } else {
    appflowy_cloud_cmd.args(["history"]);
  }

  let mut appflowy_cloud_handle = appflowy_cloud_cmd
    .spawn()
    .context("Failed to start AppFlowy-Cloud process")?;

  let mut appflowy_worker_handle = Command::new("cargo")
    .args([
      "run",
      "--manifest-path",
      "./services/appflowy-worker/Cargo.toml",
    ])
    .spawn()
    .context("Failed to start AppFlowy-Worker process")?;

  select! {
      status = appflowy_cloud_handle.wait() => {
          handle_process_exit(status?, appflowy)?;
      },
      status = appflowy_worker_handle.wait() => {
          handle_process_exit(status?, worker)?;
      }
  }

  Ok(())
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
