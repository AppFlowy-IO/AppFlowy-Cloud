# AppFlowy Cloud
- Cloud Server for AppFlowy

## Deployment
- See [deployment guide](./doc/deployment.md)

## Development

### Pre-requisites

You'll need to install:

- [Rust](https://www.rust-lang.org/tools/install)
- [Docker](https://docs.docker.com/get-docker/)

### Configuration
- copy the configurations from `dev.env` to `.env`
- edit the `.env` as required (such as SMTP configurations)

### Run with all dependencies
```bash
./build/run_local_server.sh
```

This process will execute all the dependencies and start the AppFlowy-Cloud server. Alternatively, 
you have the option to run the AppFlowy-Cloud server independently### Run the AppFlowy-Cloud

1. Run the dependency servers
```bash
docker compose --file docker-compose-dev.yml up -d
```

2. Install sqlx-cli
```bash
cargo install sqlx-cli
```

3. Run sqlx migration
```bash
sqlx database create
sqlx migrate run
cargo sqlx prepare --workspace
```

4. Run the server
```bash
cargo run
```

### Run the tests

After the server is running, you can run the tests with:

```bash
cargo test
```

### Debugging
#### Postgres
- [PgAdmin](http://localhost:5400)
- OR command line:
```bash
    export PGPASSWORD=password
    psql --host=localhost --username=postgres --port=5433
```

- Redis
```bash
    redis-cli -p 6380
```

#### Minio
- [Web UI](http://localhost:9001)

#### Portainer
- [Web UI](http://localhost:9442)
