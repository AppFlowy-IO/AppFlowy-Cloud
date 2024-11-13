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
  let appflowy_cloud_bin_name = "appflowy_cloud";
  let history = "appflowy_history";
  let worker = "appflowy_worker";

  kill_existing_process(appflowy_cloud_bin_name).await?;
  kill_existing_process(history).await?;
  kill_existing_process(worker).await?;

  let mut appflowy_cloud_cmd = Command::new("cargo")
    .args(["run", "--features", "history"])
    .spawn()
    .context("Failed to start AppFlowy-Cloud process")?;

  // let mut appflowy_history_cmd = Command::new("cargo")
  //   .args([
  //     "run",
  //     // "--features",
  //     // "verbose_log",
  //     "--manifest-path",
  //     "./services/appflowy-history/Cargo.toml",
  //   ])
  //   .spawn()
  //   .context("Failed to start AppFlowy-History process")?;

  let mut appflowy_worker_cmd = Command::new("cargo")
    .args([
      "run",
      "--manifest-path",
      "./services/appflowy-worker/Cargo.toml",
    ])
    .spawn()
    .context("Failed to start AppFlowy-Worker process")?;

  select! {
      status = appflowy_cloud_cmd.wait() => {
          handle_process_exit(status?, appflowy_cloud_bin_name)?;
      },
      // status = appflowy_history_cmd.wait() => {
      //     handle_process_exit(status?, history)?;
      // }
      status = appflowy_worker_cmd.wait() => {
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
    println!("{} exited normally.", process_name);
    Ok(())
  } else {
    Err(anyhow!("{} process failed", process_name))
  }
}
