use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct RealtimeUserImpl {
  pub uid: i64,
  pub uuid: String,
  pub device_id: String,
}

impl RealtimeUserImpl {
  pub fn new(uid: i64, uuid: String, device_id: String) -> Self {
    Self {
      uid,
      uuid,
      device_id,
    }
  }
}

impl Display for RealtimeUserImpl {
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.write_fmt(format_args!(
      "uuid:{}|device_id:{}",
      self.uuid, self.device_id,
    ))
  }
}
