use appflowy_server::application::{init_state, Application};
use appflowy_server::config::config::{get_configuration, DatabaseSetting};
use appflowy_server::state::State;
use sqlx::types::Uuid;
use sqlx::{Connection, Executor, PgConnection, PgPool};

#[derive(Clone)]
pub struct TestServer {
    pub state: State,
}

pub async fn spawn_server() -> TestServer {
    let database_name = Uuid::new_v4().to_string();
    let configuration = {
        let mut c = get_configuration().expect("Failed to read configuration.");
        c.database.database_name = database_name.clone();
        // Use a random OS port
        c.application.port = 0;
        c
    };

    let _ = configure_database(&configuration.database).await;
    let state = init_state(&configuration).await;
    let application = Application::build(configuration.clone(), state.clone())
        .await
        .expect("Failed to build application.");

    let _ = tokio::spawn(async {
        let _ = application.run_until_stopped();
    });

    TestServer { state }
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
