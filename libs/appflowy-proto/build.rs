use std::io::Result;
fn main() -> Result<()> {
  prost_build::compile_protos(&["src/proto/messages.proto"], &["src/proto"])?;
  Ok(())
}
