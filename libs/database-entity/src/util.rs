use validator::ValidationError;

pub(crate) fn validate_not_empty_str(s: &str) -> Result<(), ValidationError> {
  if s.is_empty() {
    return Err(ValidationError::new("should not be empty string"));
  }
  Ok(())
}

pub(crate) fn validate_not_empty_payload(payload: &[u8]) -> Result<(), ValidationError> {
  if payload.is_empty() {
    return Err(ValidationError::new("should not be empty payload"));
  }
  Ok(())
}

pub(crate) fn validate_int_bigger_or_equal_to_zero(i: &i64) -> Result<(), ValidationError> {
  if *i < 0 {
    return Err(ValidationError::new("should be bigger or equal to zero"));
  }
  Ok(())
}
