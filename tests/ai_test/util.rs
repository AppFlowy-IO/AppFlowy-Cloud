use fancy_regex::Regex;
use std::fs::File;
use std::io::Read;

#[allow(dead_code)]
pub(crate) fn read_text_from_asset(file_name: &str) -> String {
  let mut file = File::open(format!("./tests/ai_test/asset/{}", file_name)).unwrap();
  let mut buffer = Vec::new();
  file.read_to_end(&mut buffer).unwrap();
  String::from_utf8(buffer).unwrap()
}

pub fn extract_image_url(text: &str) -> Option<String> {
  // Define a regex pattern to match the image URL inside ![]()
  let re = Regex::new(r"!\[\]\((https?://[^\s]+)\)").unwrap();

  // Search for the first match in the text
  if let Ok(Some(captures)) = re.captures(text) {
    // Extract the matched group (the URL)
    captures.get(1).map(|m| m.as_str().to_string())
  } else {
    None
  }
}
