#!/usr/bin/env bash
set -x
set -eo pipefail

cd "$(dirname "$0")/.."

source .env
# register user 1
curl localhost:9998/signup \
	--data-raw '{"email":"'"$GOTRUE_REGISTERED_EMAIL_1"'","password":"'"$GOTRUE_REGISTERED_PASSWORD_1"'"}' \
	--header 'Content-Type: application/json'
# register user 2
curl localhost:9998/signup \
	--data-raw '{"email":"'"$GOTRUE_REGISTERED_EMAIL_2"'","password":"'"$GOTRUE_REGISTERED_PASSWORD_2"'"}' \
	--header 'Content-Type: application/json'
# register user 3
curl localhost:9998/signup \
	--data-raw '{"email":"'"$GOTRUE_REGISTERED_EMAIL_3"'","password":"'"$GOTRUE_REGISTERED_PASSWORD_3"'"}' \
	--header 'Content-Type: application/json'

