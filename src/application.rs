use actix_identity::IdentityMiddleware;
use actix_web::{dev::Server, middleware, web, web::Data, App, HttpServer};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::net::TcpListener;
use std::sync::Arc;

use crate::api::{token_scope, user_scope};

use crate::config::config::{Config, DatabaseSettings};
use crate::config::env::{domain, secret};
use crate::middleware::cors::default_cors;
use crate::state::State;

pub struct Application {
    port: u16,
    server: Server,
}

impl Application {
    pub async fn build(configuration: Config, state: State) -> Result<Self, std::io::Error> {
        let address = format!(
            "{}:{}",
            configuration.application.host, configuration.application.port
        );
        let listener = TcpListener::bind(&address)?;
        let port = listener.local_addr().unwrap().port();
        let server = run(listener, state)?;
        Ok(Self { port, server })
    }

    pub async fn run_until_stopped(self) -> Result<(), std::io::Error> {
        self.server.await
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

pub fn run(listener: TcpListener, state: State) -> Result<Server, std::io::Error> {
    let domain = domain();
    let secret: String = secret();

    let server = HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .wrap(IdentityMiddleware::default())
            .wrap(default_cors())
            .app_data(web::JsonConfig::default().limit(4096))
            .service(user_scope())
            .service(token_scope())
            .app_data(Data::new(state.clone()))
    })
    .listen(listener)?
    .run();
    Ok(server)
}

pub async fn init_state(configuration: &Config) -> State {
    let pg_pool = get_connection_pool(&configuration.database)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "Failed to connect to Postgres at {:?}.",
                configuration.database
            )
        });

    State {
        pg_pool,
        cache: Arc::new(Default::default()),
    }
}

pub async fn get_connection_pool(setting: &DatabaseSettings) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .connect_timeout(std::time::Duration::from_secs(5))
        .connect_with(setting.with_db())
        .await
}
