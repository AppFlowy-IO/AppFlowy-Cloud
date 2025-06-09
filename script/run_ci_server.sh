#!/usr/bin/env bash

#===============================================================================
# AppFlowy Cloud CI Server Runner
#===============================================================================
#
# DESCRIPTION:
#   This script builds and runs AppFlowy Cloud services using Docker Compose.
#   It supports building individual services or combinations of services,
#   with options for development or production builds.
#
# PREREQUISITES:
#   - Docker and Docker Compose installed
#   - .env file exists (copy from deploy.env and customize)
#
# USAGE:
#   ./run_ci_server.sh [SERVICE] [VERSION] [OPTIONS]
#
# SERVICES:
#   cloud           - Build only the cloud service
#   worker          - Build only the worker service  
#   admin_frontend  - Build only the admin frontend service
#   all             - Build cloud + worker services (default)
#   full            - Build all three services
#
# VERSION:
#   [version-tag]   - Docker image tag (default: "latest")
#
# ENVIRONMENT VARIABLES:
#   SKIP_BUILD=1    - Skip building, pull existing images instead
#   RELEASE_BUILD=1 - Build with release profile (optimized, slower)
#
# EXAMPLES:
#   # Build all services with latest tag (development mode)
#   ./run_ci_server.sh
#   ./run_ci_server.sh all
#
#   # Build specific service with custom version
#   ./run_ci_server.sh cloud v1.2.3
#   ./run_ci_server.sh worker v2.0.0
#   ./run_ci_server.sh admin_frontend latest
#
#   # Build all services including admin frontend
#   ./run_ci_server.sh full v1.2.3
#
#   # Skip build and use existing images
#   SKIP_BUILD=1 ./run_ci_server.sh cloud v1.2.3
#
#   # Build with production optimizations (slower but optimized)
#   RELEASE_BUILD=1 ./run_ci_server.sh full v1.2.3
#
#   # Combine options
#   SKIP_BUILD=1 ./run_ci_server.sh all latest
#
# NOTES:
#   - Script automatically updates .env with localhost URLs for testing
#   - Original .env is backed up as .env.backup
#   - Use 'cp .env.backup .env' to restore original settings
#   - Services are accessible via nginx proxy at http://localhost
#
#===============================================================================

set -eo pipefail
set -x

cd "$(dirname "$0")/.."

# Check .env
if [[ ! -f ".env" ]]; then
  echo ".env file does not exist. Please copy deploy.env to .env and update the values."
  exit 1
fi

# Parse args
SERVICE=${1:-all}           # cloud, worker, admin_frontend, all, or full
IMAGE_VERSION=${2:-latest}  # tag, defaults to "latest"

case "$SERVICE" in
  cloud|appflowy_cloud)
    BUILD_CLOUD=true
    BUILD_WORKER=false
    BUILD_ADMIN_FRONTEND=false
    ;;
  worker|appflowy_worker)
    BUILD_CLOUD=false
    BUILD_WORKER=true
    BUILD_ADMIN_FRONTEND=false
    ;;
  admin_frontend)
    BUILD_CLOUD=false
    BUILD_WORKER=false
    BUILD_ADMIN_FRONTEND=true
    ;;
  all)
    BUILD_CLOUD=true
    BUILD_WORKER=true
    BUILD_ADMIN_FRONTEND=false
    ;;
  full)
    BUILD_CLOUD=true
    BUILD_WORKER=true
    BUILD_ADMIN_FRONTEND=true
    ;;
  *)
    echo "Usage: $0 [cloud|worker|admin_frontend|all|full] [image-version]"
    exit 1
    ;;
esac

# Teardown
docker ps -q --filter "network=appflowy-cloud_default" | xargs -r docker stop
docker ps -aq --filter "network=appflowy-cloud_default" | xargs -r docker rm
docker compose down

# Build or pull
if [[ -z "${SKIP_BUILD+x}" ]]; then
  # Use debug profile by default for faster builds (set RELEASE_BUILD=1 for optimized production builds)
  if [[ -n "${RELEASE_BUILD+x}" ]]; then
    echo "Building with release profile (optimized for production)"
    BUILD_ARGS="--build-arg PROFILE=release"
  else
    echo "Building with debug profile for faster compilation (default)"
    BUILD_ARGS="--build-arg PROFILE=debug"
  fi
  
  # Build selected services (using default platform for better performance)
  $BUILD_CLOUD && docker build $BUILD_ARGS \
    -t appflowyinc/appflowy_cloud:"$IMAGE_VERSION" \
    -f Dockerfile .
  $BUILD_WORKER && docker build $BUILD_ARGS \
    -t appflowyinc/appflowy_worker:"$IMAGE_VERSION" \
    -f services/appflowy-worker/Dockerfile .
  $BUILD_ADMIN_FRONTEND && docker build $BUILD_ARGS \
    -t appflowyinc/admin_frontend:"$IMAGE_VERSION" \
    -f admin_frontend/Dockerfile .

  # Generate override for selected services
  rm -f docker-compose.override.yml  # Clean up any existing override file
  cat > docker-compose.override.yml <<EOF
version: '3'
services:
EOF
  
  if $BUILD_CLOUD; then
    cat >> docker-compose.override.yml <<EOF
  appflowy_cloud:
    image: appflowyinc/appflowy_cloud:$IMAGE_VERSION
EOF
  else
    cat >> docker-compose.override.yml <<EOF
  appflowy_cloud:
    image: appflowyinc/appflowy_cloud:latest
EOF
  fi
  
  if $BUILD_WORKER; then
    cat >> docker-compose.override.yml <<EOF
  appflowy_worker:
    image: appflowyinc/appflowy_worker:$IMAGE_VERSION
EOF
  else
    cat >> docker-compose.override.yml <<EOF
  appflowy_worker:
    image: appflowyinc/appflowy_worker:latest
EOF
  fi
  
  if $BUILD_ADMIN_FRONTEND; then
    cat >> docker-compose.override.yml <<EOF
  admin_frontend:
    image: appflowyinc/admin_frontend:$IMAGE_VERSION
EOF
  else
    cat >> docker-compose.override.yml <<EOF
  admin_frontend:
    image: appflowyinc/admin_frontend:latest
EOF
  fi

  export RUST_LOG=trace
  docker compose -f docker-compose-ci.yml -f docker-compose.override.yml up -d
  rm docker-compose.override.yml

  # Update .env file with nginx proxy URLs for local testing
  echo ""
  echo "Updating .env file with nginx proxy URLs for local testing..."
  
  # Backup original .env if it doesn't have a backup already
  if [[ ! -f ".env.backup" ]]; then
    cp .env .env.backup
    echo "Created backup: .env.backup"
  fi
  
  # Remove existing LOCALHOST_* variables and add new ones
  grep -v "^LOCALHOST_" .env | grep -v "# Local testing URLs (added by run_ci_server.sh)" > .env.tmp || true
  cat >> .env.tmp <<EOF

# Local testing URLs (added by run_ci_server.sh)
LOCALHOST_URL=http://localhost
LOCALHOST_WS=ws://localhost/ws/v1
LOCALHOST_WS_V2=ws://localhost/ws/v2
LOCALHOST_GOTRUE=http://localhost/gotrue
EOF
  
  mv .env.tmp .env
  
  echo "Updated .env file with:"
  echo "  LOCALHOST_URL=http://localhost"
  echo "  LOCALHOST_WS=ws://localhost/ws/v1"
  echo "  LOCALHOST_WS_V2=ws://localhost/ws/v2"
  echo "  LOCALHOST_GOTRUE=http://localhost/gotrue"
  echo ""
  echo "You can now run your tests. The .env file has been updated."
  echo "To restore original settings, run: cp .env.backup .env"

else
  echo "Skipping build; using existing images with tag $IMAGE_VERSION"
  export RUST_LOG=trace

  # Set versions for compose pull
  $BUILD_CLOUD && export APPFLOWY_CLOUD_VERSION="$IMAGE_VERSION"
  $BUILD_WORKER && export APPFLOWY_WORKER_VERSION="$IMAGE_VERSION"
  $BUILD_ADMIN_FRONTEND && export APPFLOWY_ADMIN_FRONTEND_VERSION="$IMAGE_VERSION"

  docker compose -f docker-compose-ci.yml pull

  if $BUILD_CLOUD; then
    echo "appflowy_cloud image version:"
    docker images appflowyinc/appflowy_cloud --format "{{.Repository}}:{{.Tag}} ({{.CreatedSince}}, {{.Size}})"
  fi

  if $BUILD_WORKER; then
    echo "appflowy_worker image version:"
    docker images appflowyinc/appflowy_worker --format "{{.Repository}}:{{.Tag}} ({{.CreatedSince}}, {{.Size}})"
  fi

  if $BUILD_ADMIN_FRONTEND; then
    echo "admin_frontend image version:"
    docker images appflowyinc/admin_frontend --format "{{.Repository}}:{{.Tag}} ({{.CreatedSince}}, {{.Size}})"
  fi

  docker compose -f docker-compose-ci.yml up -d
  
  # Update .env file with nginx proxy URLs for local testing
  echo ""
  echo "Updating .env file with nginx proxy URLs for local testing..."
  
  # Backup original .env if it doesn't have a backup already
  if [[ ! -f ".env.backup" ]]; then
    cp .env .env.backup
    echo "Created backup: .env.backup"
  fi
  
  # Remove existing LOCALHOST_* variables and add new ones
  grep -v "^LOCALHOST_" .env | grep -v "# Local testing URLs (added by run_ci_server.sh)" > .env.tmp || true
  cat >> .env.tmp <<EOF

# Local testing URLs (added by run_ci_server.sh)
LOCALHOST_URL=http://localhost
LOCALHOST_WS=ws://localhost/ws/v1
LOCALHOST_WS_V2=ws://localhost/ws/v2
LOCALHOST_GOTRUE=http://localhost/gotrue
EOF
  
  mv .env.tmp .env
  
  echo "Updated .env file with:"
  echo "  LOCALHOST_URL=http://localhost"
  echo "  LOCALHOST_WS=ws://localhost/ws/v1"
  echo "  LOCALHOST_WS_V2=ws://localhost/ws/v2"
  echo "  LOCALHOST_GOTRUE=http://localhost/gotrue"
  echo ""
  echo "You can now run your tests. The .env file has been updated."
  echo "To restore original settings, run: cp .env.backup .env"
fi