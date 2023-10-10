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
