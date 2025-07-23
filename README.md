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

- [🚀 Deployment](#deployment)
- [💻 Development](#development)
- [🐞 Debugging](#debugging)
- [⚙️ Contributing](#️contributing)

## 🚀Deployment

- See [deployment guide](./doc/DEPLOYMENT.md)

## 💻Development

### Pre-requisites

You'll need to install:

- [Rust](https://www.rust-lang.org/tools/install)
- [Docker](https://docs.docker.com/get-docker/)

### Environment Configuration

To get started, you need to set up your environment variables. We've made this easy with an interactive script:

```bash
./script/generate_env.sh
```

The script will ask you to choose between development (`dev.env`) or production (`deploy.env`) settings, then generate a
`.env` file for you. If you have sensitive values like API keys, you can put them in environment-specific secret files
and the script will safely merge them in.

#### Quick Setup with Secrets (Recommended)

**You don't need to understand all the environment variables.** For most development setups, simply:

1. Copy the development secrets template:
   ```bash
   cp env.dev.secret.example .env.dev.secret
   ```

2. Edit `.env.dev.secret` and fill in only the values you need (like API keys, passwords, etc.)

3. Run the generator:
   ```bash
   ./script/generate_env.sh
   ```

The script will automatically use your secrets file and generate a complete `.env` with sensible defaults for everything
else.

#### Manual Setup

If you prefer doing it manually, just copy one of the template files:

```bash
cp dev.env .env    # for development
```

Then edit the `.env` file with your specific settings. **Choose ONE of the following commands** to start the AppFlowy
Cloud server
locally(make sure you are in the root directory of the project):

```bash
# For new setup - RECOMMENDED FOR FIRST TIME
./script/run_local_server.sh --reset

# Basic run (interactive prompts for container management)
./script/run_local_server.sh

# With SQLx metadata preparation (useful for clean builds)
./script/run_local_server.sh --sqlx

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

## ⚙️Contributing

Any new contribution is more than welcome in this project!
If you want to know more about the development workflow or want to contribute, please visit
our [contributing guidelines](./doc/CONTRIBUTING.md) for detailed instructions!
