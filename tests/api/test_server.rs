use appflowy_server::application::{init_state, Application};
use appflowy_server::config::config::{get_configuration, DatabaseSetting};
use appflowy_server::state::State;
use appflowy_server::telemetry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;

use appflowy_server::component::auth::{RegisterResponse, HEADER_TOKEN};
use sqlx::types::Uuid;
use sqlx::{Connection, Executor, PgConnection, PgPool};

// Ensure that the `tracing` stack is only initialised once using `once_cell`
static TRACING: Lazy<()> = Lazy::new(|| {
    let level = "info".to_string();
    let mut filters = vec![];
    filters.push(format!("appflowy_server={}", level));
    filters.push(format!("hyper={}", level));

    let subscriber_name = "test".to_string();
    let subscriber = get_subscriber(subscriber_name, filters.join(","), std::io::stdout);
    init_subscriber(subscriber);
});

#[derive(Clone)]
pub struct TestServer {
    pub state: State,
    pub api_client: reqwest::Client,
    pub address: String,
    pub port: u16,
}

impl TestServer {
    pub async fn register(&self, name: &str, email: &str, password: &str) -> reqwest::Response {
        let payload = serde_json::json!({
            "name": name,
            "password": password,
            "email": email
        });
        let url = format!("{}/api/user/register", self.address);
        self.api_client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .expect("Register failed")
    }

    pub async fn login(&self, email: &str, password: &str) -> reqwest::Response {
        let payload = serde_json::json!({
            "password": password,
            "email": email
        });
        let url = format!("{}/api/user/login", self.address);
        self.api_client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .expect("Login failed")
    }

    pub async fn change_password(
        &self,
        token: String,
        current_password: &str,
        new_password: &str,
        new_password_confirm: &str,
    ) -> reqwest::Response {
        let payload = serde_json::json!({
            "current_password": current_password,
            "new_password": new_password,
            "new_password_confirm": new_password_confirm
        });
        let url = format!("{}/api/user/password", self.address);
        self.api_client
            .post(&url)
            .header(HEADER_TOKEN, token)
            .json(&payload)
            .send()
            .await
            .expect("Change password failed")
    }
}

pub async fn spawn_server() -> TestServer {
    Lazy::force(&TRACING);
    let database_name = Uuid::new_v4().to_string();
    let config = {
        let mut config = get_configuration().expect("Failed to read configuration.");
        config.database.database_name = database_name.clone();
        // Use a random OS port
        config.application.port = 0;
        config
    };

    let _ = configure_database(&config.database).await;
    let state = init_state(&config).await;
    let application = Application::build(config.clone(), state.clone())
        .await
        .expect("Failed to build application");

    let port = application.port();
    let address = format!("http://localhost:{}", port);
    let _ = tokio::spawn(async {
        let _ = application.run_until_stopped().await;
    });

    let api_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true)
        .build()
        .unwrap();

    TestServer {
        state,
        api_client,
        address,
        port,
    }
}

async fn configure_database(config: &DatabaseSetting) -> PgPool {
    // Create database
    let mut connection = PgConnection::connect_with(&config.without_db())
        .await
        .expect("Failed to connect to Postgres");
    connection
        .execute(&*format!(r#"CREATE DATABASE "{}";"#, config.database_name))
        .await
        .expect("Failed to create database.");

    // Migrate database
    let connection_pool = PgPool::connect_with(config.with_db())
        .await
        .expect("Failed to connect to Postgres.");

    sqlx::migrate!("./migrations")
        .run(&connection_pool)
        .await
        .expect("Failed to migrate the database");

    connection_pool
}

#[derive(serde::Serialize)]
pub struct TestUser {
    name: String,
    pub email: String,
    pub password: String,
}

impl TestUser {
    pub fn generate() -> Self {
        Self {
            name: "Me".to_string(),
            email: "me@appflowy.io".to_string(),
            password: "Hello@AppFlowy123".to_string(),
        }
    }

    pub async fn register(&self, test_server: &TestServer) -> String {
        let url = format!("{}/api/user/register", test_server.address);
        let resp = test_server
            .api_client
            .post(&url)
            .json(self)
            .send()
            .await
            .expect("Fail to register user");

        let bytes = resp.bytes().await.unwrap();
        let response: RegisterResponse = serde_json::from_slice(&bytes).unwrap();
        response.token
    }
}

pub async fn error_msg_from_resp(resp: reqwest::Response) -> String {
    let bytes = resp.bytes().await.unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}
