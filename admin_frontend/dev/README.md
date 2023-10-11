# Development

- Run the dev docker compose in the previous directory: `docker compose --file docker-compose-dev.yml up -d`
- cp `dev.env` to `.env`
- run the frontend: `cargo watch -x run -w .`
- web will be served at `localhost:3000`
