use std::fs::File;
use std::io::Read;

pub(crate) fn read_bytes_from_file(file_name: &str) -> Vec<u8> {
  let mut file = File::open(format!("./tests/yrs_version/files/{}", file_name)).unwrap();
  let mut buffer = Vec::new();
  file.read_to_end(&mut buffer).unwrap();
  buffer
}
