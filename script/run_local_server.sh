#!/usr/bin/env bash
#
# AppFlowy Cloud Local Development Server
# =====================================
#
# Sets up and runs a complete local development environment for AppFlowy Cloud.
# Includes PostgreSQL database, authentication services, and the AppFlowy Cloud application.
#
# USAGE:
#   ./script/run_local_server.sh             # Normal run (no DB reset)
#   ./script/run_local_server.sh --sqlx      # Run with SQLx preparation
#   ./script/run_local_server.sh --reset     # Force database reset (no prompt)
#   ./script/run_local_server.sh --reset --sqlx  # Reset DB and prepare SQLx
#
# PREREQUISITES:
#   - Docker & Docker Compose
#   - PostgreSQL client (psql)
#   - Rust & Cargo toolchain
#   - .env file (copy from dev.env)
#   - sqlx-cli (will be installed automatically if missing)
#
# INTERACTIVE PROMPTS:
#   - Stop existing containers? (default: no, data is preserved)
#   - Install sqlx-cli? (if missing, default: yes)
#
# COMMAND LINE FLAGS:
#   --sqlx     Prepare SQLx metadata (takes a few minutes)
#   --reset    Reset database schema and data (no prompt)
#
# NOTE: Database setup (reset) only runs if --reset is provided.
#
# KEY ENVIRONMENT VARIABLES:
#   SKIP_SQLX_PREPARE=true     # Skip SQLx preparation (faster restarts)
#   SKIP_APPFLOWY_CLOUD=true   # Skip AppFlowy Cloud build
#   SQLX_OFFLINE=false         # Connect to DB during build (default: true)
#
# TROUBLESHOOTING:
#   - Missing .env: cp dev.env .env
#   - Connection issues: Check Docker containers are running
#   - Build errors: Ensure Rust toolchain is installed
#   - SQLx errors: Run SQLx preparation or set SQLX_OFFLINE=false

set -x
set -eo pipefail

# Parse command line arguments
PREPARE_SQLX=false
RESET_DB=false
for arg in "$@"; do
    case $arg in
        --sqlx)
            PREPARE_SQLX=true
            shift # Remove --sqlx from processing
            ;;
        --reset)
            RESET_DB=true
            shift # Remove --reset from processing
            ;;
        *)
            echo -e "${RED}Unknown argument: $arg${NC}"
            exit 1
            ;;
    esac
done

cd "$(dirname "$0")/.."

# Color codes for better visual output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Interactive prompt functions
check_sqlx_cli() {
    if ! command -v sqlx &> /dev/null; then
        set +x
        echo -e "${RED}⚠️  sqlx-cli is not installed${NC}"
        echo -e "${YELLOW}sqlx-cli is required for database operations.${NC}"
        if prompt_yes_no "Install sqlx-cli now? (cargo install sqlx-cli)" "y"; then
            echo -e "${BLUE}Installing sqlx-cli...${NC}"
            set -x
            cargo install sqlx-cli
            set +x
            echo -e "${GREEN}✓ sqlx-cli installed successfully${NC}"
            set -x
            return 0
        else
            echo -e "${RED}Cannot proceed without sqlx-cli. Exiting.${NC}"
            exit 1
        fi
    fi
}

prompt_yes_no() {
    local message="$1"
    local default="$2"
    
    # Temporarily disable command printing
    set +x
    
    while true; do
        if [[ "$default" == "y" ]]; then
            echo -e "${CYAN}$message [Y/n]:${NC} \c"
            read response
            response=${response:-y}
        else
            echo -e "${CYAN}$message [y/N]:${NC} \c"
            read response
            response=${response:-n}
        fi
        
        case $response in
            [Yy]* ) set -x; return 0;;
            [Nn]* ) set -x; return 1;;
            * ) echo -e "${YELLOW}Please answer yes (y) or no (n).${NC}";;
        esac
    done
}

show_banner() {
    # Temporarily disable command printing
    set +x
    echo -e "${BLUE}=============================================="
    echo -e "  AppFlowy Cloud Local Development Setup"
    echo -e "===============================================${NC}"
    echo ""
    set -x
}

DB_USER="${POSTGRES_USER:=postgres}"
DB_PASSWORD="${POSTGRES_PASSWORD:=password}"
DB_PORT="${POSTGRES_PORT:=5432}"
DB_HOST="${POSTGRES_HOST:=localhost}"

# Enable SQLX offline mode by default for faster and more reliable builds
export SQLX_OFFLINE="${SQLX_OFFLINE:=true}"

show_banner

if [ ! -f .env ]; then
    echo ".env file not found in the current directory. Try: cp dev.env .env"
    exit 1
fi

# Stop and remove existing containers
if [[ "$RESET_DB" == "true" ]]; then
    # When --reset is used, automatically stop and remove containers
    echo -e "${YELLOW}Stopping and removing existing containers (--reset used)...${NC}"
    docker compose --file ./docker-compose-dev.yml down
    echo -e "${GREEN}✓ Containers stopped and removed (database data is preserved in Docker volume)${NC}"
elif prompt_yes_no "Stop and remove existing containers? (Data will be preserved)" "n"; then
    echo -e "${YELLOW}Stopping and removing existing containers...${NC}"
    docker compose --file ./docker-compose-dev.yml down
    echo -e "${GREEN}✓ Containers stopped and removed (database data is preserved in Docker volume)${NC}"
else
    echo -e "${YELLOW}Keeping existing containers running.${NC}"
    echo -e "${BLUE}Tip: You can manually stop containers with: docker compose --file ./docker-compose-dev.yml down${NC}"
fi

echo ""
echo "Starting Docker Compose services..."

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

# Generate protobuf files for collab-rt-entity crate.
# To run sqlx prepare, we need to build the collab-rt-entity crate first
./script/code_gen.sh

echo ""
if [[ "$RESET_DB" == "true" ]]; then
    check_sqlx_cli
    set +x
    echo -e "${YELLOW}Setting up database...${NC}"
    echo -e "${RED}Warning: This will reset the database schema and clear existing data in affected tables${NC}"
    echo -e "${BLUE}Creating database and running migrations (--reset used)...${NC}"
    set -x
    cargo sqlx database create && cargo sqlx migrate run
    set +x
    echo -e "${GREEN}✓ Database setup completed${NC}"
    set -x
fi

echo ""
# Interactive prompt for SQLx preparation (unless explicitly skipped)
if [[ -z "${SKIP_SQLX_PREPARE+x}" ]]; then
    if [[ "$PREPARE_SQLX" == "true" ]]; then
        check_sqlx_cli
        set +x
        echo -e "${BLUE}Preparing SQLx metadata...${NC}"
        set -x
        cargo sqlx prepare --workspace
        set +x
        echo -e "${GREEN}✓ SQLx preparation completed${NC}"
        set -x
    else
        set +x
        echo -e "${YELLOW}Skipping SQLx preparation. Use --sqlx flag to prepare SQLx metadata.${NC}"
        set -x
    fi
else
    set +x
    echo -e "${YELLOW}SQLx preparation skipped (SKIP_SQLX_PREPARE is set)${NC}"
    set -x
fi

echo ""
set +x
echo -e "${GREEN}=============================================="
echo -e "✓ AppFlowy Cloud Local Development Setup Complete!"
echo -e "==============================================${NC}"
echo ""
echo -e "${CYAN}Services running:${NC}"
echo -e "  ${YELLOW}• PostgreSQL Database:${NC} ${BLUE}localhost:${DB_PORT}${NC}"
echo -e "  ${YELLOW}• AppFlowy Cloud API:${NC} ${BLUE}localhost:9999${NC}"
echo -e "  ${YELLOW}• Authentication Service:${NC} ${BLUE}(Check docker-compose-dev.yml for ports)${NC}"
echo ""
echo -e "${CYAN}Build configuration:${NC}"
echo -e "  ${YELLOW}• SQLX_OFFLINE:${NC} ${BLUE}${SQLX_OFFLINE}${NC} (offline mode for faster builds)"
echo ""
echo -e "${CYAN}To stop all services:${NC} ${BLUE}docker compose --file ./docker-compose-dev.yml down${NC}"
echo -e "${CYAN}To view logs:${NC} ${BLUE}docker compose --file ./docker-compose-dev.yml logs -f${NC}"
echo ""
set -x

# Build AppFlowy Cloud (unless explicitly skipped)
if [[ -z "${SKIP_APPFLOWY_CLOUD+x}" ]]; then
    set +x
    echo -e "${BLUE}Building AppFlowy Cloud...${NC}"
    set -x
    cargo run --package xtask
    set +x
    echo -e "${GREEN}✓ AppFlowy Cloud build completed${NC}"
    set -x
else
    set +x
    echo -e "${YELLOW}AppFlowy Cloud build skipped (SKIP_APPFLOWY_CLOUD is set)${NC}"
    set -x
fi


# revert to require signup email verification
export GOTRUE_MAILER_AUTOCONFIRM=false
docker compose --file ./docker-compose-dev.yml up -d