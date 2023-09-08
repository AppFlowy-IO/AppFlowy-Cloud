pub enum Grant {
  Password(PasswordGrant),
  RefreshToken,
  IdToken,
  PKCE,
}

pub struct PasswordGrant {
  pub email: String,
  pub password: String,
}

impl Grant {
  pub fn type_as_str(&self) -> &str {
    match self {
      Grant::Password(_) => "password",
      Grant::RefreshToken => "refresh_token",
      Grant::IdToken => "id_token",
      Grant::PKCE => "password",
    }
  }

  pub fn json_value(&self) -> serde_json::Value {
    match self {
      Grant::Password(p) => {
        serde_json::json!({
            "email": p.email,
            "password": p.password,
        })
      },
      Grant::RefreshToken => todo!(),
      Grant::IdToken => todo!(),
      Grant::PKCE => todo!(),
    }
  }
}
