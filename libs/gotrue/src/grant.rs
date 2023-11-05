use tracing::warn;

pub enum Grant {
  Password(PasswordGrant),
  RefreshToken(RefreshTokenGrant),
  IdToken,
  PKCE,
}

pub struct PasswordGrant {
  pub email: String,
  pub password: String,
}

pub struct RefreshTokenGrant {
  pub refresh_token: String,
}

impl Grant {
  pub fn type_as_str(&self) -> &str {
    match self {
      Grant::Password(_) => "password",
      Grant::RefreshToken(_) => "refresh_token",
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
      Grant::RefreshToken(r) => {
        serde_json::json!({
            "refresh_token": r.refresh_token,
        })
      },
      Grant::IdToken => {
        warn!("id_token grant is not supported");
        serde_json::json!({ "msg": "id_token grant is not supported"})
      },
      Grant::PKCE => {
        warn!("pcke grant is not supported");
        serde_json::json!({ "msg": "pcke grant is not supported"})
      },
    }
  }
}
