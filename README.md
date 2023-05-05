## Pre-requisites

You'll need to install:

- [Rust](https://www.rust-lang.org/tools/install)
- [Docker](https://docs.docker.com/get-docker/)

Here are the Os-specific requirements:

### Windows

```bash
cargo install -f cargo-binutils
rustup component add llvm-tools-preview
```

```
cargo install --version="~0.6" sqlx-cli --no-default-features --features rustls,postgres
```

### Linux

```bash
# Ubuntu 
sudo apt-get install libssl-dev postgresql-client
# Arch 
sudo pacman -S postgresql
```

```
cargo install --version="~0.6" sqlx-cli --no-default-features --features rustls,postgres
```

### MacOS

```
cargo install --version="~0.6" sqlx-cli --no-default-features --features rustls,postgres
```

## How to build

Run `the init_db.sh` to create a Postgres database container in Docker:

```bash
./scripts/init_db.sh
```

Run the `init_redis.sh` to create a Redis container in Docker:

```bash
./scripts/init_redis.sh
```

Build the project:

```bash
cargo build
```
or you can try to run the tests in your local machine:

```bash
cargo test
```
