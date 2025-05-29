use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  // If the `PROTOC` environment variable is set, don't use vendored `protoc`
  std::env::var("PROTOC").map(|_| ()).unwrap_or_else(|_| {
    let protoc_path = protoc_bin_vendored::protoc_bin_path().expect("protoc bin path");
    let protoc_path_str = protoc_path.to_str().expect("protoc path to str");

    // Set the `PROTOC` environment variable to the path of the `protoc` binary.
    std::env::set_var("PROTOC", protoc_path_str);
  });

  let proto_files = vec!["proto/realtime.proto", "proto/collab.proto"];
  for proto_file in &proto_files {
    println!("cargo:rerun-if-changed={}", proto_file);
  }

  prost_build::Config::new()
    .out_dir("src/")
    .compile_protos(&proto_files, &["proto/"])?;

  // Run cargo fmt to format the code
  Command::new("cargo").arg("fmt").status()?;
  Ok(())
}
