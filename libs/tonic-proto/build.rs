use tonic_build::configure;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  configure()
    // .type_attribute(
    //   "history.RepeatedSnapshotMeta",
    //   "#[derive(serde::Serialize, serde::Deserialize)]",
    // )
    // .type_attribute(
    //   "history.SnapshotMeta",
    //   "#[derive(serde::Serialize, serde::Deserialize)]",
    // )
    .compile(&["proto/history.proto"], &["proto"])?;
  Ok(())
}
