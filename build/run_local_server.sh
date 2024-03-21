#!/usr/bin/env bash
set -x
set -eo pipefail

cd "$(dirname "$0")/.."

DB_USER="${POSTGRES_USER:=postgres}"
DB_PASSWORD="${POSTGRES_PASSWORD:=password}"
DB_PORT="${POSTGRES_PORT:=5432}"
DB_HOST="${POSTGRES_HOST:=localhost}"

# Stop and remove any existing containers to avoid conflicts
docker compose --file ./docker-compose-dev.yml down

# Start the Docker Compose setup
export GOTRUE_MAILER_AUTOCONFIRM=true

# Enable Google OAuth when running locally
export GOTRUE_EXTERNAL_GOOGLE_ENABLED=true

docker compose --file ./docker-compose-dev.yml up -d --build

# Keep pinging Postgres until it's ready to accept commands
ATTEMPTS=0
MAX_ATTEMPTS=30  # Adjust this value based on your needs
until PGPASSWORD="${DB_PASSWORD}" psql -h "${DB_HOST}" -U "${DB_USER}" -p "${DB_PORT}" -d "postgres" -c '\q' || [ $ATTEMPTS -eq $MAX_ATTEMPTS ]; do
  >&2 echo "Postgres is still unavailable - sleeping"
  sleep 1
  ATTEMPTS=$((ATTEMPTS+1))
done

if [ $ATTEMPTS -eq $MAX_ATTEMPTS ]; then
  >&2 echo "Failed to connect to Postgres after $MAX_ATTEMPTS attempts, exiting."
  exit 1
fi

until curl localhost:9999/health; do
  sleep 1
done

# Kill any existing instances
pkill -f appflowy_cloud || true

# Generate protobuf files for collab-rt-entity crate.
# To run sqlx prepare, we need to build the collab-rt-entity crate first
./build/code_gen.sh

# Require if there are any changes to the database schema
# To build AppFlowy-Cloud binary, we requires the .sqlx files
# To generate the .sqlx files, we need to run the following command
# After the .sqlx files are generated, we build in SQLX_OFFLINE=true
# where we don't need to connect to the database
cargo sqlx database create && cargo sqlx migrate run
if [[ -z "${SKIP_SQLX_PREPARE+x}" ]]
then
  cargo sqlx prepare --workspace
fi

# Maximum number of restart attempts
MAX_RESTARTS=5
RESTARTS=0
# Start the server and restart it on failure
while [ "$RESTARTS" -lt "$MAX_RESTARTS" ]; do
  RUST_LOG=trace RUST_BACKTRACE=full cargo run --features="ai_enable" &
  PID=$!
  wait $PID || {
    RESTARTS=$((RESTARTS+1))
    echo "Server crashed! Attempting to restart ($RESTARTS/$MAX_RESTARTS)"
    sleep 5
  }
done

if [ "$RESTARTS" -eq "$MAX_RESTARTS" ]; then
  echo "Server failed to start after $MAX_RESTARTS attempts, exiting."
  exit 1
fi


# revert to require signup email verification
export GOTRUE_MAILER_AUTOCONFIRM=false
docker compose --file ./docker-compose-dev.yml up -d
