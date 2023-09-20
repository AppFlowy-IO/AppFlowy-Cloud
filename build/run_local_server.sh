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
export GOTRUE_MAILER_AUTOCONFIRM=true
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

until curl localhost:9998/health; do
  >&2 echo "Waiting on GoTrue"
  sleep 1
done

# Kill any existing instances
pkill -f appflowy_cloud || true

# Require if there are any changes to the database schema
# To build AppFlowy-Cloud binary, we requires the .sqlx files
# To generate the .sqlx files, we need to run the following command
# After the .sqlx files are generated, we build in SQLX_OFFLINE=true
# where we don't need to connect to the database
cargo sqlx database create && cargo sqlx migrate run && cargo sqlx prepare --workspace
RUST_LOG=trace cargo run &


# sometimes the gotrue server may not be ready yet
sleep 1
source .env
# register user 1
curl localhost:9998/signup \
	--data-raw '{"email":"'"$GOTRUE_REGISTERED_EMAIL_1"'","password":"'"$GOTRUE_REGISTERED_PASSWORD_1"'"}' \
	--header 'Content-Type: application/json'
# register user 2
curl localhost:9998/signup \
	--data-raw '{"email":"'"$GOTRUE_REGISTERED_EMAIL_2"'","password":"'"$GOTRUE_REGISTERED_PASSWORD_2"'"}' \
	--header 'Content-Type: application/json'

# revert to require signup email verification
export GOTRUE_MAILER_AUTOCONFIRM=false
docker-compose --file ./docker-compose-dev.yml up -d
