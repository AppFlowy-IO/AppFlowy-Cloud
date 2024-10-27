#!/usr/bin/env bash
set -x
set -eo pipefail

cd "$(dirname "$0")/.."

# This script simulates the continuous integration (CI) environment on a local machine. It
# requires a `.env` file to be located in the project's root directory. The values in the `.env`
# file must be updated to reflect the specifications of the CI environment.
# Check if .env file exists in the current directory

if [ -f ".env" ]; then
  echo ".env file exists"
else
  echo ".env file does not exist. Copying deploy.env to .env and update the values"
  exit 1  # Exit with an error code to indicate failure
fi

# Make sure to update the test client configuration in libs/client-api-test-util/src/client.rs
# export LOCALHOST_URL="http://localhost"
# export LOCALHOST_WS_URL="ws://localhost/ws/v1"
# export LOCALHOST_GOTRUE_URL="http://localhost:gotrue"

docker compose down
docker compose -f docker-compose-ci.yml pull

# SKIP_BUILD_APPFLOWY_CLOUD=true.
if [[ -z "${SKIP_BUILD_APPFLOWY_CLOUD+x}" ]]
then
  docker build -t appflowy_cloud . && docker build -t appflowy_worker .
fi

docker compose -f docker-compose-ci.yml up -d