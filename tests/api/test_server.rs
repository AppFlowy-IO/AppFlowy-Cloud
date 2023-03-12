use appflowy_server::application::{init_state, Application};
use appflowy_server::config::config::{get_configuration, DatabaseSetting};
use appflowy_server::state::State;
use appflowy_server::telemetry::{get_subscriber, init_subscriber};
use once_cell::sync::Lazy;
use serde_json::json;
use sqlx::types::Uuid;
use sqlx::{Connection, Executor, PgConnection, PgPool};

// Ensure that the `tracing` stack is only initialised once using `once_cell`
static TRACING: Lazy<()> = Lazy::new(|| {
    let default_filter_level = "info".to_string();
    let subscriber_name = "test".to_string();
    let subscriber = get_subscriber(subscriber_name, default_filter_level, std::io::stdout);
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
            .expect("Fail to register user")
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
        .expect("Failed to build application.");

    let api_client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true)
        .build()
        .unwrap();

    let port = application.port();
    let address = format!("http://localhost:{}", port);
    let _ = tokio::spawn(async {
        let _ = application.run_until_stopped();
    });

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
