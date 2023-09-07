#!/usr/bin/env bash
set -x
set -eo pipefail

cd "$(dirname "$0")/.."

docker-compose --file ./docker-compose-dev.yml up -d --build
sqlx database create
sqlx migrate run
cargo run
