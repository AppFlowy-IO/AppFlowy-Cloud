<p align="center">
    <picture>
        <source srcset="assets/logos/appflowy_logo_white.svg" media="(prefers-color-scheme: dark)"/>
        <img src="assets/logos/appflowy_logo_black.svg"  width="500" height="200" />
    </picture>
</p>

<h4 align="center">
    <a href="https://discord.gg/9Q2xaN37tV"><img src="https://img.shields.io/badge/AppFlowy.IO-discord-orange"></a>
    <a href="https://opensource.org/licenses/AGPL-3.0"><img src="https://img.shields.io/badge/license-AGPL-purple.svg" alt="License: AGPL"></a>
</h4>

<p align="center">
    <a href="https://www.appflowy.com"><b>Website</b></a> •
    <a href="https://twitter.com/appflowy"><b>Twitter</b></a>
</p>

<p align="center">⚡ The AppFlowy Cloud written with Rust 🦀</p>

# AppFlowy Cloud

AppFlowy Cloud is part of the AppFlowy ecosystem, offering secure user authentication, file storage,
and real-time WebSocket communication for an efficient and collaborative user experience.

## Table of Contents

- [🚀 Deployment](#-deployment)
- [💻 Development](#-development)
- [🐞 Debugging](#-debugging)
- [⚙️ Contributing](#-contributing)

## 🚀 Deployment

- See [deployment guide](./doc/DEPLOYMENT.md)

## 💻 Development

### Pre-requisites

You'll need to install:

- [Rust](https://www.rust-lang.org/tools/install)
- [Docker](https://docs.docker.com/get-docker/)

### Configuration

- copy the configurations from `dev.env` to `.env`
- edit the `.env` as required (such as SMTP configurations)

Run the following command to start the AppFlowy Cloud server locally:

```bash
# Basic run (interactive prompts for container management)
./script/run_local_server.sh

# With SQLx metadata preparation (useful for clean builds)
./script/run_local_server.sh --sqlx

# With database reset (resets schema and data)
./script/run_local_server.sh --reset

# Combined: reset database and prepare SQLx metadata
./script/run_local_server.sh --reset --sqlx
```

**Interactive Features:**

- Prompts before stopping existing containers (data is preserved)
- Automatically checks for sqlx-cli and offers to install if missing
- Color-coded output for better visibility
- Clear warnings about data-affecting operations

**Command Line Flags:**

- `--sqlx`: Prepare SQLx metadata (takes a few minutes, required for clean builds)
- `--reset`: Reset database schema and data (no prompt)

**Environment Variables:**

- `SKIP_SQLX_PREPARE=true`: Skip SQLx preparation (faster restarts)
- `SKIP_APPFLOWY_CLOUD=true`: Skip AppFlowy Cloud build
- `SQLX_OFFLINE=false`: Connect to DB during build (default: true)

This process will execute all dependencies and start the AppFlowy-Cloud server with an interactive setup experience.

### Manual Setup (Step-by-Step)

If you cannot run the `run_local_server.sh` script, follow these manual steps:

#### 1. Prerequisites Check

Ensure you have installed:

- [Rust](https://www.rust-lang.org/tools/install) and Cargo toolchain
- [Docker](https://docs.docker.com/get-docker/) and Docker Compose
- PostgreSQL client (psql)
- sqlx-cli: `cargo install sqlx-cli`

#### 2. Configuration Setup

```bash
# Copy the configuration template
cp dev.env .env

# Edit the .env file as required (such as SMTP configurations)
```

#### 3. Start Docker Services

```bash
# Set environment variables
export GOTRUE_MAILER_AUTOCONFIRM=true
export GOTRUE_EXTERNAL_GOOGLE_ENABLED=true

# Start Docker Compose services
docker compose --file ./docker-compose-dev.yml up -d --build
```

#### 4. Wait for Services to Start

```bash
# Wait for PostgreSQL to be ready (adjust connection details as needed)
# Keep trying until connection succeeds
PGPASSWORD="password" psql -h "localhost" -U "postgres" -p "5432" -d "postgres" -c '\q'

# Wait for AppFlowy Cloud health check
# Keep trying until health endpoint responds
curl localhost:9999/health
```

#### 5. Database Setup (Optional)

If you need to reset/setup the database:

```bash
# Generate protobuf files for collab-rt-entity crate
./script/code_gen.sh

# Create database and run migrations
cargo sqlx database create
cargo sqlx migrate run
```

#### 6. SQLx Preparation (Optional)

If you need to prepare SQLx metadata:

```bash
# Prepare SQLx metadata (takes a few minutes)
cargo sqlx prepare --workspace
```

#### 7. Build AppFlowy Cloud

```bash
# Build and run AppFlowy Cloud
cargo run --package xtask
```

## ⚙️ Contributing

Any new contribution is more than welcome in this project!
If you want to know more about the development workflow or want to contribute, please visit
our [contributing guidelines](./doc/CONTRIBUTING.md) for detailed instructions!
