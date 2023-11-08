use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  prost_build::Config::new()
    .out_dir("src/")
    .compile_protos(&["proto/realtime.proto"], &["proto/"])?;

  // Run rustfmt on the generated files.
  let files = std::fs::read_dir("src/")?
    .filter_map(Result::ok)
    .filter(|entry| {
      entry
        .path()
        .extension()
        .map(|ext| ext == "rs")
        .unwrap_or(false)
    })
    .map(|entry| entry.path().display().to_string());

  for file in files {
    Command::new("rustfmt").arg(file).status()?;
  }
  Ok(())
}
