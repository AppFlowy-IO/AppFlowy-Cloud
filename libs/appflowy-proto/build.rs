use std::process::Command;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  // If the `PROTOC` environment variable is set, don't use vendored `protoc`
  std::env::var("PROTOC").map(|_| ()).unwrap_or_else(|_| {
    let protoc_path = protoc_bin_vendored::protoc_bin_path().expect("protoc bin path");
    let protoc_path_str = protoc_path.to_str().expect("protoc path to str");

    // Set the `PROTOC` environment variable to the path of the `protoc` binary.
    std::env::set_var("PROTOC", protoc_path_str);
  });

  let proto_files = vec![
    "proto/messages.proto",
    "proto/collab.proto",
    "proto/user.proto",
  ];
  for proto_file in &proto_files {
    println!("cargo:rerun-if-changed={}", proto_file);
  }

  prost_build::Config::new()
    .out_dir("src/pb/")
    .compile_protos(&proto_files, &["proto/"])?;

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
