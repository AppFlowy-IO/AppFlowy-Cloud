#!/usr/bin/env bash
set -x
set -eo pipefail

cd "$(dirname "$0")/.."

# temporary allow signup without email verification
export GOTRUE_MAILER_AUTOCONFIRM=true
docker-compose --file ./docker-compose-dev.yml up -d

# sometimes the gotrue server may not be ready yet
sleep 1
source .env
curl localhost:9998/signup \
	--data-raw '{"email":"'"$GOTRUE_REGISTERED_EMAIL"'","password":"'"$GOTRUE_REGISTERED_PASSWORD"'"}' \
	--header 'Content-Type: application/json'

# revert to require signup email verification
export GOTRUE_MAILER_AUTOCONFIRM=false
docker-compose --file ./docker-compose-dev.yml up -d
cargo test
