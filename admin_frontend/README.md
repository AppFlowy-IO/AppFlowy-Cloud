# Admin Frontend

## Partial Local Environment

- Go to source root folder of `AppFlowy-Cloud`
- Start running locally dependency servers: `docker compose --file docker-compose-dev.yml up -d`
- Start SQLX migrations `cargo sqlx database create && cargo sqlx migrate run && cargo sqlx prepare --workspace`
- Start AppFlowy-Cloud Server `cargo run`
- Go back to `AppFlowy-Cloud/admin_frontend` directory
- Run `cargo watch -x run -w .`, this watch for source changes, rebuild and rerun the app.

## Full Local Integration Environment

- Start the whole stack: `docker compose up -d`
- Go to [web server](localhost)
- After editing source files, do `docker compose up -d --no-deps --build admin_frontend`
- You might need to add `--force-recreate` flag for non build changes to take effect
