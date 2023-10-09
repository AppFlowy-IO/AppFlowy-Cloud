use askama::Template;

#[derive(Template)]
#[template(path = "login.html")]
pub struct Login;
