use askama::Template;

#[derive(Template)]
#[template(path = "login.html")]
pub struct Login;

#[derive(Template)]
#[template(path = "home.html")]
pub struct Home;

#[derive(Template)]
#[template(path = "admin.html")]
pub struct Admin;

#[derive(Template)]
#[template(path = "users.html")]
pub struct Users<'a> {
  pub users: &'a [gotrue_entity::User],
}

#[derive(Template)]
#[template(path = "user_details.html")]
pub struct UserDetails<'a> {
  pub user: &'a gotrue_entity::User,
}

// Any filter defined in the module `filters` is accessible in your template.
mod filters {
  pub fn default<T: std::fmt::Display>(
    input: &Option<T>,
    default_val: &str,
  ) -> ::askama::Result<String> {
    Ok(
      input
        .as_ref()
        .map(|i| i.to_string())
        .unwrap_or_else(|| default_val.to_string()),
    )
  }
}
