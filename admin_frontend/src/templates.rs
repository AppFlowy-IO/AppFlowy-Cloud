use askama::Template;

#[derive(Template)]
#[template(path = "components/change_password.html")]
pub struct ChangePassword;

#[derive(Template)]
#[template(path = "pages/login.html")]
pub struct Login;

// #[derive(Template)]
// #[template(path = "login.html")]
// pub struct Login;

#[derive(Template)]
#[template(path = "pages/home.html")]
pub struct Home<'a> {
  pub email: &'a str,
  pub is_admin: bool,
}

#[derive(Template)]
#[template(path = "components/create_user.html")]
pub struct CreateUser;

#[derive(Template)]
#[template(path = "pages/admin_home.html")]
pub struct Admin<'a> {
  pub email: &'a str,
}

#[derive(Template)]
#[template(path = "components/admin_users.html")]
pub struct AdminUsers<'a> {
  pub users: &'a [gotrue_entity::dto::User],
}

#[derive(Template)]
#[template(path = "components/user_details.html")]
pub struct UserDetails<'a> {
  pub user: &'a gotrue_entity::dto::User,
}

#[derive(Template)]
#[template(path = "components/admin_user_details.html")]
pub struct AdminUserDetails<'a> {
  pub user: &'a gotrue_entity::dto::User,
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
