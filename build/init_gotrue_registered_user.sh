#!/usr/bin/env bash

cd "$(dirname "$0")"
set -e

cd gotrue

sed -i 's/GOTRUE_MAILER_AUTOCONFIRM="false"/GOTRUE_MAILER_AUTOCONFIRM="true"/' .env.docker
sleep 5 # wait for server to restart

# Create user without confirmation
curl localhost:9999/signup \
	--header 'Content-Type: application/json' \
	--data-raw '{"email":"xigahi8979@tipent.com", "password": "Hello123!"}'

sed -i 's/GOTRUE_MAILER_AUTOCONFIRM="true"/GOTRUE_MAILER_AUTOCONFIRM="false"/' .env.docker
sleep 5 # wait for server to restart
