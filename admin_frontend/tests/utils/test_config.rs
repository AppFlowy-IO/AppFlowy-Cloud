pub struct TestConfig {
  pub hostname: String,
}

impl TestConfig {
  pub fn from_env() -> Self {
    dotenvy::dotenv().ok();

    let hostname =
      std::env::var("ADMIN_FRONTEND_TEST_HOSTNAME").unwrap_or("http://localhost:3000".to_string());
    TestConfig { hostname }
  }
}
