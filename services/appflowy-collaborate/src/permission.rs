#[derive(Debug)]
pub enum CollabUserId<'a> {
  UserId(&'a i64),
  UserUuid(&'a uuid::Uuid),
}

impl<'a> From<&'a i64> for CollabUserId<'a> {
  fn from(uid: &'a i64) -> Self {
    CollabUserId::UserId(uid)
  }
}

impl<'a> From<&'a uuid::Uuid> for CollabUserId<'a> {
  fn from(uid: &'a uuid::Uuid) -> Self {
    CollabUserId::UserUuid(uid)
  }
}
