use askama::Template;

#[derive(Template)]
#[template(path = "login.html")]
pub struct Login;

#[derive(Template)]
#[template(path = "home.html")]
pub struct Home;
