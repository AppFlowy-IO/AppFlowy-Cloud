#!/usr/bin/env bash
set -x
set -eo pipefail

cd "$(dirname "$0")/.."

DB_USER="${POSTGRES_USER:=postgres}"
DB_PASSWORD="${POSTGRES_PASSWORD:=password}"
DB_PORT="${POSTGRES_PORT:=5433}"
DB_HOST="${POSTGRES_HOST:=localhost}"

# Stop and remove any existing containers to avoid conflicts
docker-compose --file ./docker-compose-dev.yml down

# Start the Docker Compose setup
docker-compose --file ./docker-compose-dev.yml up -d --build

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

# Kill any existing instances
pkill -f appflowy_cloud || true

cargo run
