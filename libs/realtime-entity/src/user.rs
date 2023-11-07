use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RealtimeUserImpl {
  pub uid: i64,
  pub device_id: String,
}

impl RealtimeUserImpl {
  pub fn new(uid: i64, device_id: String) -> Self {
    Self { uid, device_id }
  }
}

impl Display for RealtimeUserImpl {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "uid:{}|device_id:{}",
      self.uid, self.device_id,
    ))
  }
}
