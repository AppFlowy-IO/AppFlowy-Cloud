# AppFlowy Cloud
- Cloud Server for AppFlowy

## Quick Deploy (Docker Compose)

### Environmental Variables before starting
- you can set it explicitly(below) or in a `.env` file (use `example.env`) as template
```bash
# authentication key, change this and keep the key safe and secret
GOTRUE_JWT_SECRET=secret_auth_pass

# enabled by default, if you dont want need email confirmation, set to false
GOTRUE_MAILER_AUTOCONFIRM=true

# if you enable mail confirmation, you need to set the SMTP configuration below
GOTRUE_SMTP_HOST=smtp.gmail.com
GOTRUE_SMTP_PORT=465
GOTRUE_SMTP_USER=email_sender@some_company.com
GOTRUE_SMTP_PASS=email_sender_password
GOTRUE_SMTP_ADMIN_EMAIL=comp_admin@@some_company.com

# Change 'localhost' to the public host of machine that is running on.
# This is for email confirmation link
API_EXTERNAL_URL=http://localhost:9998
```
- additional settings can be modified in `docker-compose.yml`

### Start Cloud Server
```bash
docker-compose up -d
```

### Ports
Host Server is required to expose the following Ports:
- `8000`
- `9998`

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
./build/init_database.sh
```

Run the `init_redis.sh` to create a Redis container in Docker:

```bash
./build/init_redis.sh
```

Build the project:

```bash
cargo build
```
or you can try to run the tests in your local machine:

```bash
cargo test
```
