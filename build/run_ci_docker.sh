#!/usr/bin/env bash
set -x
set -eo pipefail

cd "$(dirname "$0")/.."

# This script is used to simulate the CI environment locally. It requires the local .env file
# to be present in the root directory of the project. And the values in the .env file should be
# updated to match the CI environment.

# Check if .env file exists in the current directory
if [ -f ".env" ]; then
  echo ".env file exists"
else
  echo ".env file does not exist. Copying deploy.env to .env and update the values"
  exit 1  # Exit with an error code to indicate failure
fi

export LOCALHOST_URL="http://localhost"
export LOCALHOST_WS_URL="ws://localhost/ws"
export LOCALHOST_GOTRUE_URL="http://localhost:gotrue"

docker compose down
docker compose -f docker-compose-ci.yml pull

# SKIP_BUILD_APPFLOWY_CLOUD=true.
if [[ -z "${SKIP_BUILD_APPFLOWY_CLOUD+x}" ]]
then
  docker build -t appflowy_cloud .
fi

docker compose -f docker-compose-ci.yml up -d