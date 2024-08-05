use std::fs::File;
use std::io::Read;

pub(crate) fn read_text_from_asset(file_name: &str) -> String {
  let mut file = File::open(format!("./tests/ai_test/asset/{}", file_name)).unwrap();
  let mut buffer = Vec::new();
  file.read_to_end(&mut buffer).unwrap();
  String::from_utf8(buffer).unwrap()
}
