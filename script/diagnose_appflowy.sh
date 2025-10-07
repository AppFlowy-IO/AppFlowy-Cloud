#!/bin/bash

# =============================================================================
# AppFlowy Cloud - Diagnostic Tool
# =============================================================================
# This script diagnoses common issues with AppFlowy Cloud docker-compose
# deployments, particularly login and authentication problems.
#
# Usage: ./script/diagnose_appflowy.sh [OPTIONS]
#
# Options:
#   -h, --help              Show help message
#   -v, --verbose           Verbose output (show all checks)
#   -q, --quiet             Minimal output (errors only)
#   -f, --compose-file FILE Specify docker-compose file
#   -o, --output FILE       Save report to specific file
#   -l, --logs              Include full log analysis
#   -s, --skip-logs         Skip log analysis (faster)
#   --no-color              Disable colored output
#   --json                  Output in JSON format
#   --quick                 Quick mode (skip slow checks)
#
# Features:
# - Container health checks
# - Configuration validation
# - JWT secret verification
# - Database connectivity
# - Log analysis
# - Actionable recommendations
# =============================================================================

set -e  # Exit on any error

# ==================== INITIALIZATION ====================

# Color codes for better output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Global variables
SCRIPT_VERSION="1.0.0"
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COMPOSE_FILE=""
COMPOSE_CMD=""
OUTPUT_FILE=""
VERBOSE=false
QUIET=false
INCLUDE_LOGS=false
SKIP_LOGS=false
NO_COLOR=false
JSON_OUTPUT=false
QUICK_MODE=false

# Check results
declare -a ISSUES=()
declare -a WARNINGS=()
declare -a SUCCESSES=()

# Service version info
APPFLOWY_CLOUD_VERSION=""
ADMIN_FRONTEND_VERSION=""
DEPLOYMENT_TYPE=""

# Timestamp for report
TIMESTAMP=$(date +"%Y%m%d_%H%M%S")
REPORT_FILE="$PROJECT_ROOT/appflowy_diagnostic_$TIMESTAMP.log"

# ==================== UTILITY FUNCTIONS ====================

# Function to print colored output
print_header() {
    if [[ "$NO_COLOR" == "true" ]]; then
        echo "================================="
        echo "$1"
        echo "================================="
    else
        echo -e "${BLUE}=================================${NC}"
        echo -e "${BLUE}$1${NC}"
        echo -e "${BLUE}=================================${NC}"
    fi
}

print_success() {
    [[ "$QUIET" == "true" ]] && return
    if [[ "$NO_COLOR" == "true" ]]; then
        echo "✓ $1"
    else
        echo -e "${GREEN}✓ $1${NC}"
    fi
    SUCCESSES+=("$1")
}

print_warning() {
    if [[ "$NO_COLOR" == "true" ]]; then
        echo "⚠ $1"
    else
        echo -e "${YELLOW}⚠ $1${NC}"
    fi
    WARNINGS+=("$1")
}

print_error() {
    if [[ "$NO_COLOR" == "true" ]]; then
        echo "✗ $1"
    else
        echo -e "${RED}✗ $1${NC}"
    fi
    ISSUES+=("$1")
}

print_info() {
    [[ "$QUIET" == "true" ]] && return
    if [[ "$NO_COLOR" == "true" ]]; then
        echo "ℹ $1"
    else
        echo -e "${CYAN}ℹ $1${NC}"
    fi
}

print_verbose() {
    [[ "$VERBOSE" != "true" ]] && return
    if [[ "$NO_COLOR" == "true" ]]; then
        echo "  $1"
    else
        echo -e "${PURPLE}  $1${NC}"
    fi
}

# Function to mask sensitive data
mask_sensitive() {
    local value="$1"
    local length=${#value}
    if [[ $length -le 4 ]]; then
        echo "***"
    else
        echo "${value:0:2}***${value: -2}"
    fi
}

# Function to show help
show_help() {
    cat << EOF
AppFlowy Cloud Diagnostic Tool v${SCRIPT_VERSION}

Usage: $0 [OPTIONS]

This tool diagnoses common issues with AppFlowy Cloud docker-compose
deployments, particularly login and authentication problems.

Options:
  -h, --help              Show this help message
  -v, --verbose           Verbose output (show all checks)
  -q, --quiet             Minimal output (errors only)
  -f, --compose-file FILE Specify docker-compose file
  -o, --output FILE       Save report to specific file
  -l, --logs              Include full log analysis
  -s, --skip-logs         Skip log analysis (faster)
  --no-color              Disable colored output
  --json                  Output in JSON format
  --quick                 Quick mode (skip slow checks)

Examples:
  $0                      # Basic diagnostic
  $0 -v -l                # Verbose with logs
  $0 --quick              # Quick check
  $0 -f docker-compose-dev.yml  # Custom compose file

EOF
}

# ==================== ARGUMENT PARSING ====================

parse_arguments() {
    while [[ $# -gt 0 ]]; do
        case $1 in
            -h|--help)
                show_help
                exit 0
                ;;
            -v|--verbose)
                VERBOSE=true
                shift
                ;;
            -q|--quiet)
                QUIET=true
                shift
                ;;
            -f|--compose-file)
                COMPOSE_FILE="$2"
                shift 2
                ;;
            -o|--output)
                OUTPUT_FILE="$2"
                shift 2
                ;;
            -l|--logs)
                INCLUDE_LOGS=true
                shift
                ;;
            -s|--skip-logs)
                SKIP_LOGS=true
                shift
                ;;
            --no-color)
                NO_COLOR=true
                shift
                ;;
            --json)
                JSON_OUTPUT=true
                NO_COLOR=true
                shift
                ;;
            --quick)
                QUICK_MODE=true
                SKIP_LOGS=true
                shift
                ;;
            *)
                echo "Unknown option: $1"
                show_help
                exit 1
                ;;
        esac
    done
}

# ==================== ENVIRONMENT DETECTION ====================

check_docker() {
    print_verbose "Checking Docker installation..."

    if ! command -v docker &> /dev/null; then
        print_error "Docker is not installed or not in PATH"
        return 1
    fi

    local docker_version=$(docker --version 2>/dev/null | awk '{print $3}' | sed 's/,//')
    print_success "Docker version: $docker_version"

    # Check if Docker daemon is running
    if ! docker info &> /dev/null; then
        print_error "Docker daemon is not running"
        return 1
    fi

    print_verbose "Docker daemon is running"
    return 0
}

check_docker_compose() {
    print_verbose "Checking Docker Compose installation..."

    # Try docker compose (v2)
    if docker compose version &> /dev/null; then
        local compose_version=$(docker compose version --short 2>/dev/null)
        print_success "Docker Compose: v$compose_version (v2)"
        echo "docker compose"
        return 0
    fi

    # Try docker-compose (v1)
    if command -v docker-compose &> /dev/null; then
        local compose_version=$(docker-compose --version 2>/dev/null | awk '{print $3}' | sed 's/,//')
        print_success "Docker Compose: $compose_version (v1)"
        echo "docker-compose"
        return 0
    fi

    print_error "Docker Compose is not installed"
    return 1
}

detect_compose_file() {
    print_verbose "Detecting docker-compose file..."

    if [[ -n "$COMPOSE_FILE" ]]; then
        if [[ ! -f "$PROJECT_ROOT/$COMPOSE_FILE" ]]; then
            print_error "Specified compose file not found: $COMPOSE_FILE"
            return 1
        fi
        print_success "Using specified compose file: $COMPOSE_FILE"
        return 0
    fi

    # Check for common compose files
    if [[ -f "$PROJECT_ROOT/docker-compose.yml" ]]; then
        COMPOSE_FILE="docker-compose.yml"
        print_success "Found compose file: docker-compose.yml (production)"
        return 0
    elif [[ -f "$PROJECT_ROOT/docker-compose-dev.yml" ]]; then
        COMPOSE_FILE="docker-compose-dev.yml"
        print_success "Found compose file: docker-compose-dev.yml (development)"
        return 0
    else
        print_error "No docker-compose file found"
        return 1
    fi
}

check_env_file() {
    print_verbose "Checking for .env file..."

    if [[ ! -f "$PROJECT_ROOT/.env" ]]; then
        print_error ".env file not found. Run ./script/generate_env.sh first"
        return 1
    fi

    print_success ".env file found"
    return 0
}

# ==================== CONTAINER STATUS CHECKS ====================

get_compose_command() {
    # Cache the compose command to avoid repeated calls
    if [[ -z "$COMPOSE_CMD" ]]; then
        # Call check_docker_compose without printing (it already printed in Phase 1)
        local compose_cmd=""
        if docker compose version &> /dev/null; then
            compose_cmd="docker compose"
        elif command -v docker-compose &> /dev/null; then
            compose_cmd="docker-compose"
        else
            print_error "Docker Compose is not installed"
            return 1
        fi
        COMPOSE_CMD="$compose_cmd -f $PROJECT_ROOT/$COMPOSE_FILE"
    fi
    echo "$COMPOSE_CMD"
}

check_container_status() {
    print_verbose "Checking container status..."

    local compose_cmd=$(get_compose_command)
    local services="postgres redis gotrue appflowy_cloud admin_frontend nginx"

    # Check if any containers exist
    local container_count=$($compose_cmd ps -a -q 2>/dev/null | wc -l)
    if [[ $container_count -eq 0 ]]; then
        print_warning "No containers found. Run 'docker compose up -d' to start services"
        return 0
    fi

    for service in $services; do
        # Check if service is defined in compose file
        if ! $compose_cmd config --services 2>/dev/null | grep -q "^${service}$"; then
            print_verbose "Service '$service' not defined in compose file"
            continue
        fi

        # Get container status
        local status=$($compose_cmd ps -q $service 2>/dev/null)
        if [[ -z "$status" ]]; then
            print_warning "$service: Not created"
            continue
        fi

        local container_status=$(docker inspect --format='{{.State.Status}}' $status 2>/dev/null)
        local restart_count=$(docker inspect --format='{{.RestartCount}}' $status 2>/dev/null)
        local started_at=$(docker inspect --format='{{.State.StartedAt}}' $status 2>/dev/null)

        case "$container_status" in
            running)
                if [[ "$restart_count" -gt 3 ]]; then
                    print_warning "$service: Running but has restarted $restart_count times"
                else
                    print_success "$service: Running (restarts: $restart_count)"
                fi
                ;;
            exited)
                print_error "$service: Exited"
                ;;
            restarting)
                print_error "$service: Restarting (restart count: $restart_count)"
                ;;
            *)
                print_warning "$service: Status unknown ($container_status)"
                ;;
        esac
    done
}

check_service_versions() {
    print_verbose "Checking service versions..."

    local compose_cmd=$(get_compose_command)

    # Check AppFlowy Cloud version
    local appflowy_cloud_container=$($compose_cmd ps -q appflowy_cloud 2>/dev/null)
    if [[ -n "$appflowy_cloud_container" ]]; then
        # Extract version from logs - look for "Using AppFlowy Cloud version:X.X.X"
        APPFLOWY_CLOUD_VERSION=$(docker logs "$appflowy_cloud_container" 2>&1 | grep -oE 'Using AppFlowy Cloud version:[0-9]+\.[0-9]+\.[0-9]+' | head -1 | sed 's/Using AppFlowy Cloud version://')

        if [[ -n "$APPFLOWY_CLOUD_VERSION" ]]; then
            print_success "AppFlowy Cloud version: $APPFLOWY_CLOUD_VERSION"
        else
            print_verbose "AppFlowy Cloud: Version not found in logs"
        fi

        # Check deployment type
        DEPLOYMENT_TYPE=$(docker logs "$appflowy_cloud_container" 2>&1 | grep -oE 'deployment:[a-z-]+' | head -1 | sed 's/deployment://')
        if [[ -n "$DEPLOYMENT_TYPE" ]]; then
            print_verbose "Deployment type: $DEPLOYMENT_TYPE"
        fi
    else
        print_verbose "AppFlowy Cloud: Container not running"
    fi

    # Check Admin Frontend version
    local admin_frontend_container=$($compose_cmd ps -q admin_frontend 2>/dev/null)
    if [[ -n "$admin_frontend_container" ]]; then
        # Extract version from logs - look for "Version: X.X.X"
        ADMIN_FRONTEND_VERSION=$(docker logs "$admin_frontend_container" 2>&1 | grep -E '^Version:' | head -1 | awk '{print $2}')

        if [[ -n "$ADMIN_FRONTEND_VERSION" ]]; then
            print_success "Admin Frontend version: $ADMIN_FRONTEND_VERSION"
        else
            print_verbose "Admin Frontend: Version not found in logs"
        fi
    else
        print_verbose "Admin Frontend: Container not running"
    fi

    # Check GoTrue version (from image tag)
    local gotrue_container=$($compose_cmd ps -q gotrue 2>/dev/null)
    if [[ -n "$gotrue_container" ]]; then
        local gotrue_image=$(docker inspect --format='{{.Config.Image}}' "$gotrue_container" 2>/dev/null)
        if [[ -n "$gotrue_image" ]]; then
            print_verbose "GoTrue image: $gotrue_image"
        fi
    fi

    return 0
}

# ==================== HEALTH ENDPOINT CHECKS ====================

check_health_endpoint() {
    local service_name="$1"
    local url="$2"
    local timeout="${3:-5}"

    print_verbose "Checking health endpoint: $url"

    local response=$(curl -s -f -m "$timeout" "$url" 2>/dev/null)
    local exit_code=$?

    if [[ $exit_code -eq 0 ]]; then
        print_success "$service_name health check: OK"
        print_verbose "Response: $response"
        return 0
    else
        case $exit_code in
            7)
                print_error "$service_name health check: Connection refused"
                ;;
            28)
                print_error "$service_name health check: Timeout after ${timeout}s"
                ;;
            *)
                print_error "$service_name health check: Failed (exit code: $exit_code)"
                ;;
        esac
        return 1
    fi
}

check_health_endpoints() {
    print_verbose "Checking service health endpoints..."

    # Check if containers are running first
    local compose_cmd=$(get_compose_command)
    local container_count=$($compose_cmd ps -q 2>/dev/null | wc -l)
    if [[ $container_count -eq 0 ]]; then
        print_warning "No containers running - skipping health checks"
        return 0
    fi

    # Determine if we're in dev or prod mode by checking ports
    local gotrue_port=$($compose_cmd config 2>/dev/null | grep -A 10 "gotrue:" | grep "9999:" | head -1 | awk -F: '{print $1}' | grep -o "[0-9]*$")

    if [[ -n "$gotrue_port" ]]; then
        # Development mode - ports exposed
        check_health_endpoint "GoTrue" "http://localhost:9999/health" 5
        check_health_endpoint "AppFlowy Cloud" "http://localhost:8000/health" 5
    else
        # Production mode - via nginx
        print_verbose "Production mode: Services accessed via nginx reverse proxy"
        # Try through nginx if it's configured
        if curl -s http://localhost/gotrue/health &>/dev/null; then
            check_health_endpoint "GoTrue (via nginx)" "http://localhost/gotrue/health" 5
            # Also try to check appflowy cloud through nginx
            if curl -s http://localhost/api/health &>/dev/null; then
                check_health_endpoint "AppFlowy Cloud (via nginx)" "http://localhost/api/health" 5
            fi
        else
            print_warning "Cannot access services via nginx - check if nginx is running"
        fi
    fi

    # Check database from container
    local pg_status=$($compose_cmd ps -q postgres 2>/dev/null)
    if [[ -n "$pg_status" ]]; then
        local pg_check=$($compose_cmd exec -T postgres pg_isready -U postgres 2>/dev/null)
        if [[ $? -eq 0 ]]; then
            print_success "PostgreSQL: $pg_check"
        else
            print_error "PostgreSQL: Not ready"
        fi
    else
        print_warning "PostgreSQL: Container not running"
    fi

    # Check Redis
    local redis_status=$($compose_cmd ps -q redis 2>/dev/null)
    if [[ -n "$redis_status" ]]; then
        local redis_check=$($compose_cmd exec -T redis redis-cli ping 2>/dev/null)
        if [[ "$redis_check" == "PONG" ]]; then
            print_success "Redis: PONG"
        else
            print_error "Redis: Not responding"
        fi
    else
        print_warning "Redis: Container not running"
    fi
}

# ==================== CONFIGURATION VALIDATION ====================

load_env_vars() {
    if [[ -f "$PROJECT_ROOT/.env" ]]; then
        # Load env vars without exporting
        set -a
        source "$PROJECT_ROOT/.env"
        set +a
        return 0
    fi
    return 1
}

# Extract the scheme portion of a URL (http/https) for comparison.
extract_url_scheme() {
    local url="$1"
    printf '%s\n' "$url" | awk -F:// 'NF > 1 {print tolower($1)}'
}

extract_url_host() {
    local url="$1"
    printf '%s\n' "$url" | sed -n 's#^[^:]*://\([^/]*\).*#\1#p' | cut -d':' -f1
}

extract_url_path() {
    local url="$1"
    printf '%s\n' "$url" | sed -n 's#^[^:]*://[^/]*\(.*\)#\1#p'
}

# Compare the configured scheme with what the server actually returns.
check_url_scheme_alignment() {
    local label="$1"
    local url="$2"

    local expected_scheme
    expected_scheme=$(extract_url_scheme "$url")

    local host
    host=$(extract_url_host "$url")

    if [[ -z "$expected_scheme" ]]; then
        print_warning "$label: URL is missing http/https scheme ($url)"
        return 1
    fi

    local curl_output
    if ! curl_output=$(curl -sS -o /dev/null -D - --max-time 5 "$url" 2>&1); then
        local error_reason=$(echo "$curl_output" | tail -n 1)
        if echo "$error_reason" | grep -qi "Could not resolve host"; then
            if [[ -n "$host" && "$host" != "localhost" && "$host" != "127.0.0.1" && "$host" != "::1" && "$host" != "[::1]" && "$host" != *.* ]]; then
                print_info "$label: Host '$host' is not reachable from the local host (this is expected for internal docker hostnames)"
                return 0
            fi
        fi
        if [[ "$expected_scheme" == "https" ]]; then
            print_warning "$label: HTTPS request failed (${error_reason:-curl error})"
            print_warning "  Fix: Ensure TLS is configured or update the URL to http:// if TLS is disabled"
        else
            print_warning "$label: HTTP request failed (${error_reason:-curl error})"
            print_warning "  Fix: Confirm nginx listens on http:// or update the URL to https:// if TLS is enforced"
        fi
        return 1
    fi

    local status_code=$(echo "$curl_output" | head -n 1 | awk '{print $2}')
    local location_header=$(echo "$curl_output" | tr -d '\r' | grep -i '^Location:' | tail -n 1 | awk '{print $2}')

    if [[ -n "$location_header" ]]; then
        local redirect_scheme
        redirect_scheme=$(extract_url_scheme "$location_header")

        if [[ -n "$redirect_scheme" && "$redirect_scheme" != "$expected_scheme" ]]; then
            print_warning "$label: Configured $expected_scheme:// but server redirects to $redirect_scheme:// ($status_code -> $location_header)"
            print_warning "  Fix: Update the URL scheme in .env to match the redirect target"
            return 1
        fi
    fi

    local scheme_upper
    scheme_upper=$(printf '%s' "$expected_scheme" | tr '[:lower:]' '[:upper:]')
    print_verbose "$label: ${scheme_upper:-UNKNOWN} endpoint reachable (HTTP ${status_code:-unknown})"
    return 0
}

check_websocket_url() {
    local label="$1"
    local ws_url="$2"
    local base_url="$3"

    local ws_scheme
    ws_scheme=$(extract_url_scheme "$ws_url")

    if [[ -z "$ws_scheme" ]]; then
        print_warning "$label: Missing ws/wss scheme ($ws_url)"
        return 1
    fi

    if [[ "$ws_scheme" == "http" || "$ws_scheme" == "https" ]]; then
        print_error "$label: Expected ws:// or wss:// but found $ws_scheme://"
        print_error "  Fix: Update the WebSocket URL to use ws/wss"
        return 1
    fi

    local path
    path=$(extract_url_path "$ws_url")

    if [[ -z "$path" ]]; then
        print_warning "$label: URL has no path; expected /ws or /ws/v1"
    elif [[ "$path" != /ws* ]]; then
        print_warning "$label: Unexpected WebSocket path '$path' (expected to start with /ws)"
    fi

    if [[ -n "$base_url" ]]; then
        local base_scheme
        base_scheme=$(extract_url_scheme "$base_url")
        local ws_host
        ws_host=$(extract_url_host "$ws_url")

        if [[ "$base_scheme" == "https" && "$ws_scheme" != "wss" ]]; then
            # Only error for non-localhost deployments
            if [[ "$ws_host" != "localhost" && "$ws_host" != "127.0.0.1" ]]; then
                print_error "$label: Base URL uses https but WebSocket URL is $ws_scheme:// (CRITICAL)"
                print_error "  Fix: Use wss:// when the site is served over https"
                print_error "  Update .env: APPFLOWY_WS_BASE_URL=wss://yourdomain.com/ws/v2"
                print_error "  Or update .env: WS_SCHEME=wss (if using variable substitution)"
                return 1
            else
                print_verbose "$label: Localhost deployment - scheme mismatch acceptable for local testing"
            fi
        elif [[ "$base_scheme" == "http" && "$ws_scheme" != "ws" ]]; then
            print_warning "$label: Base URL uses http but WebSocket URL is $ws_scheme://"
            print_warning "  Fix: Use ws:// when the site is served over http"
        fi
    fi

    return 0
}

check_nginx_websocket_config() {
    print_verbose "Checking nginx WebSocket configuration..."

    local nginx_conf_files=(
        "$PROJECT_ROOT/nginx/nginx.conf"
        "$PROJECT_ROOT/nginx.conf"
        "$PROJECT_ROOT/nginx/conf.d/default.conf"
    )

    local nginx_conf=""
    for conf_file in "${nginx_conf_files[@]}"; do
        if [[ -f "$conf_file" ]]; then
            nginx_conf="$conf_file"
            break
        fi
    done

    if [[ -z "$nginx_conf" ]]; then
        print_warning "Nginx config file not found (checked common locations)"
        return 1
    fi

    print_verbose "Found nginx config: $nginx_conf"

    # Check for WebSocket upgrade headers in location blocks that proxy to AppFlowy
    local has_ws_location=false
    local has_upgrade_header=false
    local has_connection_header=false
    local has_http_version=false
    local ws_location_found=false
    local current_location=""

    # Look for location blocks that handle WebSocket (/ws or upstream appflowy_cloud)
    while IFS= read -r line; do
        # Detect location blocks for WebSocket paths
        if echo "$line" | grep -E '^\s*location\s+.*(/ws|/api)' &>/dev/null; then
            ws_location_found=true
            current_location=$(echo "$line" | grep -oE 'location\s+[^{]+' | sed 's/location //')
            print_verbose "Found WebSocket-related location: $current_location"
        fi

        # Check for WebSocket upgrade headers within relevant location blocks
        if [[ "$ws_location_found" == "true" ]]; then
            if echo "$line" | grep -E '^\s*proxy_set_header\s+Upgrade\s+\$http_upgrade' &>/dev/null; then
                has_upgrade_header=true
                print_verbose "  ✓ Found: proxy_set_header Upgrade \$http_upgrade"
            fi

            if echo "$line" | grep -iE '^\s*proxy_set_header\s+Connection\s+.*(upgrade|connection_upgrade)' &>/dev/null; then
                has_connection_header=true
                print_verbose "  ✓ Found: proxy_set_header Connection upgrade/\$connection_upgrade"
            fi

            if echo "$line" | grep -E '^\s*proxy_http_version\s+1\.1' &>/dev/null; then
                has_http_version=true
                print_verbose "  ✓ Found: proxy_http_version 1.1"
            fi

            # Reset when location block ends
            if echo "$line" | grep -E '^\s*}' &>/dev/null; then
                ws_location_found=false
            fi
        fi

        # Also check for general upstream blocks
        if echo "$line" | grep -E 'upstream\s+(appflowy_cloud|appflowy)' &>/dev/null; then
            has_ws_location=true
        fi
    done < "$nginx_conf"

    # Validate results
    local has_issues=false

    if [[ "$has_upgrade_header" != "true" ]]; then
        print_error "Nginx: Missing WebSocket Upgrade header (CRITICAL for WSS)"
        print_error "  Fix: Add to nginx WebSocket location block:"
        print_error "    proxy_set_header Upgrade \$http_upgrade;"
        has_issues=true
    else
        print_success "Nginx: WebSocket Upgrade header configured"
    fi

    if [[ "$has_connection_header" != "true" ]]; then
        print_error "Nginx: Missing WebSocket Connection header (CRITICAL for WSS)"
        print_error "  Fix: Add to nginx WebSocket location block:"
        print_error "    proxy_set_header Connection \"upgrade\";"
        has_issues=true
    else
        print_success "Nginx: WebSocket Connection header configured"
    fi

    if [[ "$has_http_version" != "true" ]]; then
        print_error "Nginx: Missing HTTP/1.1 version (CRITICAL for WSS)"
        print_error "  Fix: Add to nginx WebSocket location block:"
        print_error "    proxy_http_version 1.1;"
        has_issues=true
    else
        print_success "Nginx: HTTP/1.1 version configured"
    fi

    if [[ "$has_issues" == "true" ]]; then
        print_error ""
        print_error "Example nginx WebSocket configuration:"
        print_error "  location /ws {"
        print_error "    proxy_pass http://appflowy_cloud:8000;"
        print_error "    proxy_http_version 1.1;"
        print_error "    proxy_set_header Upgrade \$http_upgrade;"
        print_error "    proxy_set_header Connection \"upgrade\";"
        print_error "    proxy_set_header Host \$host;"
        print_error "    proxy_set_header X-Real-IP \$remote_addr;"
        print_error "  }"
        return 1
    fi

    return 0
}

check_ssl_certificate() {
    print_verbose "Checking SSL/TLS certificate configuration..."

    if ! load_env_vars; then
        return 1
    fi

    local base_url="${APPFLOWY_BASE_URL}"
    local base_scheme
    base_scheme=$(extract_url_scheme "$base_url")

    # Only check SSL if using HTTPS
    if [[ "$base_scheme" != "https" ]]; then
        print_verbose "SSL check skipped (not using HTTPS)"
        return 0
    fi

    local host
    host=$(extract_url_host "$base_url")

    if [[ -z "$host" || "$host" == "localhost" || "$host" == "127.0.0.1" ]]; then
        print_verbose "SSL check skipped (localhost deployment)"
        return 0
    fi

    print_verbose "Checking SSL certificate for: $host"

    # Check if openssl is available
    if ! command -v openssl &> /dev/null; then
        print_warning "SSL: openssl not found, cannot verify certificate"
        return 0
    fi

    # Try to get certificate info
    local cert_info
    cert_info=$(echo | openssl s_client -servername "$host" -connect "${host}:443" 2>/dev/null | openssl x509 -noout -dates 2>/dev/null)

    if [[ $? -ne 0 || -z "$cert_info" ]]; then
        print_error "SSL: Cannot retrieve certificate for $host"
        print_error "  Fix: Ensure valid SSL/TLS certificate is installed"
        print_error "  For Let's Encrypt: certbot certonly --standalone -d $host"
        return 1
    fi

    # Extract expiry date
    local not_after
    not_after=$(echo "$cert_info" | grep "notAfter=" | cut -d'=' -f2)

    if [[ -n "$not_after" ]]; then
        # Convert to epoch for comparison
        local expiry_epoch
        expiry_epoch=$(date -j -f "%b %d %T %Y %Z" "$not_after" +%s 2>/dev/null || date -d "$not_after" +%s 2>/dev/null)
        local now_epoch
        now_epoch=$(date +%s)

        if [[ -n "$expiry_epoch" ]]; then
            local days_until_expiry=$(( (expiry_epoch - now_epoch) / 86400 ))

            if [[ $days_until_expiry -lt 0 ]]; then
                print_error "SSL: Certificate EXPIRED $((days_until_expiry * -1)) days ago"
                print_error "  Fix: Renew SSL certificate immediately"
                return 1
            elif [[ $days_until_expiry -lt 7 ]]; then
                print_warning "SSL: Certificate expires in $days_until_expiry days"
                print_warning "  Fix: Renew SSL certificate soon"
            elif [[ $days_until_expiry -lt 30 ]]; then
                print_warning "SSL: Certificate expires in $days_until_expiry days"
            else
                print_success "SSL: Certificate valid (expires in $days_until_expiry days)"
            fi
        fi
    fi

    # Test actual HTTPS connection
    local https_test
    https_test=$(curl -sS -o /dev/null -w "%{http_code}" --max-time 5 "$base_url" 2>&1)

    if [[ $? -ne 0 ]]; then
        if echo "$https_test" | grep -qi "SSL certificate problem"; then
            print_error "SSL: Certificate verification failed"
            print_error "  Fix: Check certificate chain and CA certificates"
            return 1
        elif echo "$https_test" | grep -qi "SSL"; then
            print_error "SSL: Connection failed - $https_test"
            return 1
        fi
    fi

    return 0
}

check_production_https_websocket() {
    print_verbose "Checking production HTTPS/WSS configuration..."

    if ! load_env_vars; then
        return 1
    fi

    local base_url="${APPFLOWY_BASE_URL}"
    local base_scheme
    base_scheme=$(extract_url_scheme "$base_url")
    local base_host
    base_host=$(extract_url_host "$base_url")

    # Only check for production deployments (non-localhost with HTTPS)
    if [[ "$base_scheme" != "https" ]]; then
        print_verbose "Not using HTTPS - skipping production WSS check"
        return 0
    fi

    if [[ "$base_host" == "localhost" || "$base_host" == "127.0.0.1" ]]; then
        print_verbose "Localhost deployment - skipping production WSS check"
        return 0
    fi

    print_verbose "Production HTTPS deployment detected: $base_url"

    # Check WebSocket URL scheme
    local ws_url="${APPFLOWY_WS_BASE_URL:-${APPFLOWY_WEBSOCKET_BASE_URL}}"
    if [[ -z "$ws_url" ]]; then
        print_error "Production HTTPS: WebSocket URL not configured"
        print_error "  Fix: Set APPFLOWY_WS_BASE_URL=wss://$base_host/ws/v2 in .env"
        return 1
    fi

    local ws_scheme
    ws_scheme=$(extract_url_scheme "$ws_url")

    if [[ "$ws_scheme" != "wss" ]]; then
        print_error "Production HTTPS: WebSocket URL is $ws_scheme:// but MUST be wss:// (CRITICAL)"
        print_error "  Issue: Browsers block insecure WebSocket (ws://) connections from HTTPS pages"
        print_error "  This causes: 'WebSocket connection failed' errors and app hangs after login"
        print_error "  Fix: Update .env file:"
        print_error "    APPFLOWY_WS_BASE_URL=wss://$base_host/ws/v2"
        print_error "  Or if using WS_SCHEME variable:"
        print_error "    WS_SCHEME=wss"
        print_error "  Then restart: docker compose restart appflowy_cloud nginx"
        return 1
    fi

    print_success "Production HTTPS: WebSocket correctly configured with wss://"

    # Verify nginx SSL is configured
    local nginx_conf_files=(
        "$PROJECT_ROOT/nginx/nginx.conf"
        "$PROJECT_ROOT/nginx.conf"
    )

    local nginx_conf=""
    for conf_file in "${nginx_conf_files[@]}"; do
        if [[ -f "$conf_file" ]]; then
            nginx_conf="$conf_file"
            break
        fi
    done

    if [[ -n "$nginx_conf" ]]; then
        # Check if nginx has SSL configured
        if grep -E '^\s*listen\s+443\s+ssl' "$nginx_conf" &>/dev/null; then
            print_success "Production HTTPS: Nginx SSL listener configured (port 443)"
        else
            print_warning "Production HTTPS: Nginx may not have SSL configured"
            print_warning "  Check nginx config has: listen 443 ssl;"
        fi

        # Check for SSL certificate paths
        if grep -E '^\s*ssl_certificate\s+' "$nginx_conf" &>/dev/null; then
            local cert_path=$(grep -E '^\s*ssl_certificate\s+' "$nginx_conf" | head -1 | awk '{print $2}' | tr -d ';')
            print_verbose "SSL certificate path: $cert_path"
        else
            print_warning "Production HTTPS: No SSL certificate configured in nginx"
        fi
    fi

    return 0
}

check_websocket_cors_headers() {
    print_verbose "Checking CORS and security headers for WebSocket..."

    local nginx_conf_files=(
        "$PROJECT_ROOT/nginx/nginx.conf"
        "$PROJECT_ROOT/nginx.conf"
        "$PROJECT_ROOT/nginx/conf.d/default.conf"
    )

    local nginx_conf=""
    for conf_file in "${nginx_conf_files[@]}"; do
        if [[ -f "$conf_file" ]]; then
            nginx_conf="$conf_file"
            break
        fi
    done

    if [[ -z "$nginx_conf" ]]; then
        print_verbose "Nginx config not found, skipping CORS check"
        return 0
    fi

    # Check for problematic CORS headers that might block WebSocket
    local has_cors_block=false
    local has_upgrade_insecure=false
    local ws_location_found=false

    while IFS= read -r line; do
        # Detect WebSocket location blocks
        if echo "$line" | grep -E '^\s*location\s+.*(/ws|/api)' &>/dev/null; then
            ws_location_found=true
        fi

        if [[ "$ws_location_found" == "true" ]]; then
            # Check for CORS headers in WebSocket locations (can be problematic)
            if echo "$line" | grep -E 'add_header.*Access-Control-Allow-Origin' &>/dev/null; then
                has_cors_block=true
                print_verbose "Found CORS header in WebSocket location"
            fi

            # Check for upgrade-insecure-requests (can block WSS)
            if echo "$line" | grep -E 'upgrade-insecure-requests' &>/dev/null; then
                has_upgrade_insecure=true
            fi

            if echo "$line" | grep -E '^\s*}' &>/dev/null; then
                ws_location_found=false
            fi
        fi
    done < "$nginx_conf"

    # Check if there are any security headers that might interfere
    if grep -E '^\s*add_header.*Content-Security-Policy.*upgrade-insecure-requests' "$nginx_conf" &>/dev/null; then
        print_warning "Security: Content-Security-Policy with upgrade-insecure-requests may affect WebSocket"
        print_warning "  Fix: Ensure CSP allows WebSocket connections: connect-src 'self' ws: wss:;"
    fi

    print_success "CORS/Security: No blocking headers detected"
    return 0
}

check_scheme_consistency() {
    if ! load_env_vars; then
        return 1
    fi

    local scheme="${SCHEME}"
    if [[ -z "$scheme" ]]; then
        print_verbose "SCHEME variable not set (defaults depend on compose file)"
        return 0
    fi

    local scheme_lower=$(echo "$scheme" | tr '[:upper:]' '[:lower:]')
    if [[ "$scheme_lower" != "http" && "$scheme_lower" != "https" ]]; then
        print_warning "SCHEME has unexpected value '$scheme' (expected http or https)"
        return 1
    fi

    local base_url="${APPFLOWY_BASE_URL}"
    local base_scheme
    base_scheme=$(extract_url_scheme "$base_url")

    if [[ -n "$base_scheme" ]]; then
        if [[ "$base_scheme" != "$scheme_lower" ]]; then
            print_warning "SCHEME=$scheme differs from APPFLOWY_BASE_URL scheme ($base_scheme)"
            print_warning "  Fix: Align SCHEME with the protocol actually served by nginx"
        else
            print_verbose "SCHEME aligns with APPFLOWY_BASE_URL"
        fi
    else
        print_verbose "SCHEME is set but APPFLOWY_BASE_URL scheme could not be determined"
    fi

    return 0
}

check_duplicate_env_keys() {
    local keys=(
        "APPFLOWY_BASE_URL"
        "API_EXTERNAL_URL"
        "APPFLOWY_GOTRUE_BASE_URL"
        "APPFLOWY_WS_BASE_URL"
        "APPFLOWY_WEBSOCKET_BASE_URL"
        "SCHEME"
    )

    if [[ ! -f "$PROJECT_ROOT/.env" ]]; then
        return 0
    fi

    for key in "${keys[@]}"; do
        local matches
        matches=$(grep -n "^${key}=" "$PROJECT_ROOT/.env" || true)
        if [[ -n "$matches" ]]; then
            local count
            count=$(echo "$matches" | wc -l | tr -d ' ')
            if [[ "$count" -gt 1 ]]; then
                print_warning "${key} defined $count times in .env (duplicate definitions detected)"
                if [[ "$VERBOSE" == "true" ]]; then
                    print_verbose "  Lines: $(echo "$matches" | cut -d: -f1 | tr '\n' ' ')"
                fi
            fi
        fi
    done
}

check_legacy_env_vars() {
    if [[ ! -f "$PROJECT_ROOT/.env" ]]; then
        return 0
    fi

    local legacy
    legacy=$(grep -E '^AF_[A-Z0-9_]+=' "$PROJECT_ROOT/.env" || true)
    if [[ -n "$legacy" ]]; then
        print_warning "Detected deprecated AF_* environment variables; update to APPFLOWY_* equivalents"
        if [[ "$VERBOSE" == "true" ]]; then
            while IFS= read -r line; do
                print_verbose "  $line"
            done <<< "$legacy"
        fi
    fi
}

check_gotrue_configuration() {
    if ! load_env_vars; then
        return 1
    fi

    local base_url="${APPFLOWY_BASE_URL}"
    local site_url="${GOTRUE_SITE_URL}"
    local uri_allow_list="${GOTRUE_URI_ALLOW_LIST}"

    if [[ -z "$site_url" ]]; then
        print_info "GOTRUE_SITE_URL is not set (docker-compose defaults to appflowy-flutter:// for desktop/mobile deep links)"
    elif [[ -n "$base_url" ]]; then
        local base_scheme
        base_scheme=$(extract_url_scheme "$base_url")
        local site_scheme
        site_scheme=$(extract_url_scheme "$site_url")
        if [[ "$site_scheme" == "appflowy-flutter" ]]; then
            print_verbose "GOTRUE_SITE_URL configured for desktop/mobile deep link ($site_url)"
        else
            if [[ -n "$base_scheme" && -n "$site_scheme" && "$base_scheme" != "$site_scheme" ]]; then
                print_warning "GOTRUE_SITE_URL scheme ($site_scheme) differs from APPFLOWY_BASE_URL ($base_scheme)"
            fi
            if [[ $site_url != ${base_url}* ]]; then
                print_warning "GOTRUE_SITE_URL ($site_url) does not start with APPFLOWY_BASE_URL ($base_url)"
            fi
        fi
    fi

    if [[ -n "$uri_allow_list" ]]; then
        if [[ "$uri_allow_list" != *"appflowy-flutter://"* ]]; then
            print_warning "GOTRUE_URI_ALLOW_LIST missing appflowy-flutter:// callbacks"
        fi
        if [[ -n "$base_url" && "$uri_allow_list" != *"$base_url"* ]]; then
            print_warning "GOTRUE_URI_ALLOW_LIST does not include $base_url"
        fi
    else
        print_info "GOTRUE_URI_ALLOW_LIST is not set (optional, but required for desktop/mobile deep links and OAuth callbacks)"
    fi

    return 0
}

check_jwt_secrets() {
    print_verbose "Validating JWT secret configuration..."

    if ! load_env_vars; then
        print_error "Cannot load .env file"
        return 1
    fi

    local gotrue_secret="${GOTRUE_JWT_SECRET}"
    local appflowy_secret="${APPFLOWY_GOTRUE_JWT_SECRET}"

    # Source code analysis (src/config/config.rs line 213):
    # - APPFLOWY_GOTRUE_JWT_SECRET: Optional, defaults to "hello456" in AppFlowy Cloud code
    # Docker compose analysis (line 117):
    # - APPFLOWY_GOTRUE_JWT_SECRET=${GOTRUE_JWT_SECRET} in container
    # This means:
    # 1. .env only needs GOTRUE_JWT_SECRET
    # 2. Docker compose auto-sets APPFLOWY_GOTRUE_JWT_SECRET from GOTRUE_JWT_SECRET
    # 3. If neither is set, AppFlowy Cloud uses default "hello456"

    # Determine effective AppFlowy secret
    if [[ -z "$appflowy_secret" ]]; then
        # If APPFLOWY_GOTRUE_JWT_SECRET not in .env, docker-compose sets it from GOTRUE_JWT_SECRET
        # If that's also empty, code defaults to "hello456"
        if [[ -n "$gotrue_secret" ]]; then
            appflowy_secret="$gotrue_secret"
            print_verbose "APPFLOWY_GOTRUE_JWT_SECRET not set - will use GOTRUE_JWT_SECRET via docker-compose"
        else
            appflowy_secret="hello456"
            print_info "JWT secrets not configured - using default: $(mask_sensitive "$appflowy_secret")"
            print_info "  Recommended: Set GOTRUE_JWT_SECRET in .env for production"
        fi
    fi

    # GOTRUE_JWT_SECRET is required for GoTrue container (line 69 docker-compose.yml)
    if [[ -z "$gotrue_secret" ]]; then
        print_error "GOTRUE_JWT_SECRET is not set in .env (REQUIRED)"
        print_error "  GoTrue container requires this for authentication"
        print_error "  Fix: Add to .env file:"
        print_error "    GOTRUE_JWT_SECRET=$appflowy_secret"
        return 1
    fi

    # Check if they match
    if [[ "$gotrue_secret" != "$appflowy_secret" ]]; then
        print_error "JWT secrets do not match (CRITICAL)"
        print_error "  GOTRUE_JWT_SECRET: $(mask_sensitive "$gotrue_secret")"
        print_error "  APPFLOWY_GOTRUE_JWT_SECRET: $(mask_sensitive "$appflowy_secret")"
        print_error "  Fix: Ensure both match in .env:"
        print_error "    GOTRUE_JWT_SECRET=$gotrue_secret"
        print_error "    # APPFLOWY_GOTRUE_JWT_SECRET not needed (auto-set from GOTRUE_JWT_SECRET)"
        return 1
    fi

    print_success "JWT secrets match: $(mask_sensitive "$gotrue_secret")"
    return 0
}

check_database_urls() {
    print_verbose "Validating database URL configuration..."

    if ! load_env_vars; then
        return 1
    fi

    local gotrue_db="${GOTRUE_DATABASE_URL}"
    local appflowy_db="${APPFLOWY_DATABASE_URL}"

    if [[ -z "$gotrue_db" ]]; then
        print_error "GOTRUE_DATABASE_URL is not set"
        return 1
    fi

    if [[ -z "$appflowy_db" ]]; then
        print_error "APPFLOWY_DATABASE_URL is not set"
        return 1
    fi

    # Extract database host from URLs
    local gotrue_host=$(echo "$gotrue_db" | sed -n 's|.*@\([^:/]*\).*|\1|p')
    local appflowy_host=$(echo "$appflowy_db" | sed -n 's|.*@\([^:/]*\).*|\1|p')

    if [[ "$gotrue_host" != "$appflowy_host" ]]; then
        print_warning "Database hosts differ: GoTrue($gotrue_host) vs AppFlowy($appflowy_host)"
    else
        print_success "Database hosts match: $gotrue_host"
    fi

    print_verbose "GOTRUE_DATABASE_URL: $(echo $gotrue_db | sed 's|://.*@|://***:***@|')"
    print_verbose "APPFLOWY_DATABASE_URL: $(echo $appflowy_db | sed 's|://.*@|://***:***@|')"

    return 0
}

check_base_urls() {
    print_verbose "Validating base URL configuration..."

    if ! load_env_vars; then
        return 1
    fi

    local base_url="${APPFLOWY_BASE_URL}"
    local api_external="${API_EXTERNAL_URL}"
    local gotrue_base="${APPFLOWY_GOTRUE_BASE_URL}"
    local ws_url_primary="${APPFLOWY_WS_BASE_URL}"
    local ws_url_legacy="${APPFLOWY_WEBSOCKET_BASE_URL}"
    local effective_ws_url="${ws_url_primary:-$ws_url_legacy}"
    local web_url="${APPFLOWY_WEB_URL}"

    # APPFLOWY_WEB_URL is REQUIRED (src/config/config.rs line 274-275)
    # It's the only variable without a default that causes startup failure
    if [[ -z "$web_url" ]]; then
        print_error "APPFLOWY_WEB_URL is not set (REQUIRED)"
        print_error "  AppFlowy Cloud will fail to start without this"
        print_error "  Fix: Add to .env file:"
        print_error "    APPFLOWY_WEB_URL=\${APPFLOWY_BASE_URL}"
        print_error "  For custom web deployment:"
        print_error "    APPFLOWY_WEB_URL=https://your-web-domain.com"
        return 1
    else
        print_success "APPFLOWY_WEB_URL: $web_url"
    fi

    # APPFLOWY_BASE_URL has default in code, but typically set in .env for production
    if [[ -z "$base_url" ]]; then
        print_info "APPFLOWY_BASE_URL not set - check docker-compose for required services"
    else
        print_success "APPFLOWY_BASE_URL: $base_url"
        check_url_scheme_alignment "APPFLOWY_BASE_URL" "$base_url"
    fi

    # API_EXTERNAL_URL typically derived from APPFLOWY_BASE_URL in .env
    if [[ -z "$api_external" ]]; then
        print_verbose "API_EXTERNAL_URL not set (usually set via .env template)"
    else
        print_success "API_EXTERNAL_URL: $api_external"
        check_url_scheme_alignment "API_EXTERNAL_URL" "$api_external"
    fi

    # APPFLOWY_GOTRUE_BASE_URL has default "http://localhost:9999" (src/config/config.rs line 212)
    if [[ -z "$gotrue_base" ]]; then
        print_verbose "APPFLOWY_GOTRUE_BASE_URL not set - using default: http://localhost:9999"
    else
        print_success "APPFLOWY_GOTRUE_BASE_URL: $gotrue_base"
        check_url_scheme_alignment "APPFLOWY_GOTRUE_BASE_URL" "$gotrue_base"
        local gotrue_scheme
        gotrue_scheme=$(extract_url_scheme "$gotrue_base")
        local gotrue_host
        gotrue_host=$(extract_url_host "$gotrue_base")
        if [[ -n "$gotrue_host" && "$gotrue_host" != *.* && "$gotrue_scheme" == "https" ]]; then
            print_warning "APPFLOWY_GOTRUE_BASE_URL uses https with internal host '$gotrue_host'"
            print_warning "  Fix: Use http://gotrue:9999 when referencing the container internally"
        fi
    fi

    if [[ -n "$ws_url_primary" && -n "$ws_url_legacy" ]]; then
        print_warning "Both APPFLOWY_WS_BASE_URL and APPFLOWY_WEBSOCKET_BASE_URL are set; APPFLOWY_WS_BASE_URL takes precedence"
    fi

    if [[ -n "$effective_ws_url" ]]; then
        local ws_label
        if [[ -n "$ws_url_primary" ]]; then
            ws_label="APPFLOWY_WS_BASE_URL"
        else
            ws_label="APPFLOWY_WEBSOCKET_BASE_URL"
        fi
        print_success "$ws_label: $effective_ws_url"
        check_websocket_url "$ws_label" "$effective_ws_url" "$base_url"
    else
        print_warning "WebSocket base URL not set (APPFLOWY_WS_BASE_URL or APPFLOWY_WEBSOCKET_BASE_URL)"
    fi

    return 0
}

check_admin_credentials() {
    print_verbose "Checking admin credentials configuration..."

    if ! load_env_vars; then
        return 1
    fi

    local admin_email="${GOTRUE_ADMIN_EMAIL}"
    local admin_password="${GOTRUE_ADMIN_PASSWORD}"

    if [[ -z "$admin_email" ]]; then
        print_warning "GOTRUE_ADMIN_EMAIL is not set"
    else
        print_success "Admin email configured: $admin_email"
    fi

    if [[ -z "$admin_password" ]]; then
        print_warning "GOTRUE_ADMIN_PASSWORD is not set"
    else
        print_success "Admin password configured: $(mask_sensitive "$admin_password")"
    fi

    return 0
}

check_smtp_configuration() {
    print_verbose "Checking SMTP configuration for emails..."

    if ! load_env_vars; then
        return 1
    fi

    local gotrue_smtp_host="${GOTRUE_SMTP_HOST}"
    local gotrue_smtp_port="${GOTRUE_SMTP_PORT}"
    local gotrue_smtp_user="${GOTRUE_SMTP_USER}"

    local appflowy_smtp_host="${APPFLOWY_MAILER_SMTP_HOST}"
    local appflowy_smtp_port="${APPFLOWY_MAILER_SMTP_PORT}"
    local appflowy_smtp_username="${APPFLOWY_MAILER_SMTP_USERNAME}"
    local appflowy_smtp_email="${APPFLOWY_MAILER_SMTP_EMAIL}"

    local has_gotrue_smtp=false
    local has_appflowy_smtp=false
    local has_issues=false

    # Check if GOTRUE SMTP is configured
    if [[ -n "$gotrue_smtp_host" && -n "$gotrue_smtp_port" ]]; then
        has_gotrue_smtp=true
        print_success "GOTRUE SMTP configured: $gotrue_smtp_host:$gotrue_smtp_port"
        print_verbose "  Used for: Authentication emails (signup, password reset, magic links)"
    fi

    # Check if APPFLOWY MAILER SMTP is configured
    if [[ -n "$appflowy_smtp_host" && -n "$appflowy_smtp_port" ]]; then
        has_appflowy_smtp=true
        print_success "APPFLOWY_MAILER SMTP configured: $appflowy_smtp_host:$appflowy_smtp_port"
        print_verbose "  Used for: Workspace invitations, sharing notifications"
    fi

    # Critical check: GOTRUE configured but APPFLOWY not configured
    if [[ "$has_gotrue_smtp" == "true" && "$has_appflowy_smtp" == "false" ]]; then
        print_error "SMTP Configuration Incomplete (CRITICAL for workspace sharing)"
        print_error "  Issue: GOTRUE_SMTP is configured but APPFLOWY_MAILER_SMTP is not"
        print_error "  Impact: Users CANNOT share workspaces or send invitations"
        print_error "  GOTRUE_SMTP is only for auth emails (signup, password reset)"
        print_error "  APPFLOWY_MAILER_SMTP is required for workspace invitations"
        print_error ""
        print_error "  Fix: Add these to .env file:"
        print_error "    APPFLOWY_MAILER_SMTP_HOST=$gotrue_smtp_host"
        print_error "    APPFLOWY_MAILER_SMTP_PORT=$gotrue_smtp_port"
        if [[ -n "$gotrue_smtp_user" ]]; then
            print_error "    APPFLOWY_MAILER_SMTP_USERNAME=$gotrue_smtp_user"
            print_error "    APPFLOWY_MAILER_SMTP_EMAIL=$gotrue_smtp_user"
        fi
        print_error "    APPFLOWY_MAILER_SMTP_PASSWORD=<your_smtp_password>"
        print_error "    APPFLOWY_MAILER_SMTP_TLS_KIND=wrapper  # or 'required'"
        print_error ""
        print_error "  Then restart: docker compose restart appflowy_cloud"
        has_issues=true
    fi

    # Warning: Neither configured
    if [[ "$has_gotrue_smtp" == "false" && "$has_appflowy_smtp" == "false" ]]; then
        print_info "SMTP: Not configured (email features disabled)"
        print_info "  Without SMTP, the following features won't work:"
        print_info "    - Workspace invitations (users cannot share workspaces)"
        print_info "    - Email notifications"
        print_info "    - Password reset emails (if GOTRUE_MAILER_AUTOCONFIRM=false)"
        print_info "  To enable email features, configure both:"
        print_info "    - GOTRUE_SMTP_* (for authentication emails)"
        print_info "    - APPFLOWY_MAILER_SMTP_* (for workspace invitations)"
    fi

    # Success: Both configured
    if [[ "$has_gotrue_smtp" == "true" && "$has_appflowy_smtp" == "true" ]]; then
        print_success "SMTP: Fully configured for all email features"

        # Check if credentials are set
        if [[ -z "$appflowy_smtp_username" || -z "$appflowy_smtp_email" ]]; then
            print_warning "APPFLOWY_MAILER_SMTP_USERNAME or APPFLOWY_MAILER_SMTP_EMAIL not set"
        fi
    fi

    # Additional validation: Check TLS kind
    local tls_kind="${APPFLOWY_MAILER_SMTP_TLS_KIND}"
    if [[ "$has_appflowy_smtp" == "true" ]]; then
        if [[ -z "$tls_kind" ]]; then
            print_warning "APPFLOWY_MAILER_SMTP_TLS_KIND not set (defaults may not work)"
            print_warning "  Recommended values: 'wrapper' (port 465), 'required' (port 587)"
        else
            print_verbose "TLS Kind: $tls_kind"
        fi
    fi

    if [[ "$has_issues" == "true" ]]; then
        return 1
    fi

    return 0
}

# ==================== FUNCTIONAL TESTS ====================

check_minio_storage() {
    print_verbose "Checking Minio/S3 file storage..."

    local compose_cmd=$(get_compose_command)

    # Check if minio container is running
    local minio_status=$($compose_cmd ps -q minio 2>/dev/null)
    if [[ -z "$minio_status" ]]; then
        print_warning "Minio: Container not running (file storage unavailable)"
        return 1
    fi

    # Check if bucket exists
    local bucket_check=$($compose_cmd exec -T minio sh -c 'ls -la /data/appflowy' 2>/dev/null)
    if [[ $? -eq 0 ]]; then
        local file_count=$(echo "$bucket_check" | wc -l)
        print_success "Minio: Bucket 'appflowy' exists with data"
        print_verbose "Files in bucket: $file_count items"
    else
        print_error "Minio: Bucket 'appflowy' not found or inaccessible"
        return 1
    fi

    return 0
}

check_database_tables() {
    print_verbose "Checking database schema..."

    local compose_cmd=$(get_compose_command)

    # Check if postgres container is running
    local pg_status=$($compose_cmd ps -q postgres 2>/dev/null)
    if [[ -z "$pg_status" ]]; then
        print_warning "PostgreSQL: Container not running"
        return 1
    fi

    # Check for essential AppFlowy tables
    local required_tables=(
        "af_workspace"
        "af_collab"
        "af_workspace_member"
        "af_user"
    )

    local missing_tables=()
    for table in "${required_tables[@]}"; do
        local table_exists=$($compose_cmd exec -T postgres psql -U postgres -d postgres -tAc "SELECT EXISTS (SELECT FROM information_schema.tables WHERE table_name = '$table');" 2>/dev/null)
        if [[ "$table_exists" != "t" ]]; then
            missing_tables+=("$table")
        fi
    done

    if [[ ${#missing_tables[@]} -gt 0 ]]; then
        print_error "Database: Missing tables: ${missing_tables[*]}"
        return 1
    else
        print_success "Database: All essential tables exist"
    fi

    return 0
}

check_api_endpoints() {
    print_verbose "Checking API endpoints..."

    # Test public API endpoint (should return 401 for unauthorized)
    local api_response=$(curl -s -o /dev/null -w "%{http_code}" http://localhost/api/workspace 2>/dev/null)

    if [[ "$api_response" == "401" ]]; then
        print_success "API: Workspace endpoint responding (requires auth)"
    elif [[ "$api_response" == "404" ]]; then
        print_error "API: Workspace endpoint not found (routing issue)"
        return 1
    elif [[ -z "$api_response" ]] || [[ "$api_response" == "000" ]]; then
        print_error "API: Cannot connect to API endpoints"
        return 1
    else
        print_success "API: Workspace endpoint responding (HTTP $api_response)"
    fi

    # Test GoTrue signup endpoint
    local gotrue_response=$(curl -s -o /dev/null -w "%{http_code}" http://localhost/gotrue/signup 2>/dev/null)

    if [[ "$gotrue_response" == "400" ]] || [[ "$gotrue_response" == "422" ]]; then
        print_success "GoTrue: Signup endpoint responding (requires data)"
    elif [[ "$gotrue_response" == "404" ]]; then
        print_error "GoTrue: Signup endpoint not found"
        return 1
    else
        print_verbose "GoTrue: Signup endpoint HTTP $gotrue_response"
    fi

    return 0
}

check_websocket_endpoint() {
    print_verbose "Checking WebSocket endpoint..."

    local compose_cmd=$(get_compose_command)

    # Load WebSocket URL from .env to test the correct endpoint
    if ! load_env_vars; then
        print_warning "WebSocket: Cannot load .env configuration"
        return 1
    fi

    # Extract path from APPFLOWY_WEBSOCKET_BASE_URL (e.g., ws://localhost/ws/v2 -> /ws/v2)
    local ws_path=$(echo "${APPFLOWY_WEBSOCKET_BASE_URL}" | sed 's|^[^/]*//[^/]*/|/|')
    if [[ -z "$ws_path" ]]; then
        ws_path="/ws/v1"  # Default fallback
    fi

    print_verbose "Testing WebSocket path: $ws_path"

    # Check if this is v2 (requires workspace_id)
    if [[ "$ws_path" == "/ws/v2" ]]; then
        # Get a workspace ID from database
        local pg_status=$($compose_cmd ps -q postgres 2>/dev/null)
        if [[ -n "$pg_status" ]]; then
            local workspace_id=$($compose_cmd exec -T postgres psql -U postgres -d postgres -tAc "SELECT workspace_id FROM af_workspace LIMIT 1;" 2>/dev/null | tr -d '[:space:]')

            if [[ -n "$workspace_id" ]] && [[ "$workspace_id" != "" ]]; then
                ws_path="/ws/v2/${workspace_id}"
                print_verbose "Using workspace ID: $workspace_id"
            else
                print_info "WebSocket v2: No workspaces yet (will be available after first workspace created)"
                return 0
            fi
        else
            print_warning "WebSocket v2: Cannot verify (database not accessible)"
            return 0
        fi
    fi

    # Try to connect to WebSocket (will fail auth but proves endpoint exists)
    local ws_response=$(curl -s --max-time 5 \
        -H "Connection: Upgrade" \
        -H "Upgrade: websocket" \
        -H "Sec-WebSocket-Version: 13" \
        -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
        http://localhost${ws_path} 2>&1)

    # Check if response contains token/authorization error (this means endpoint works!)
    if echo "$ws_response" | grep -qi "token\|authorization\|Missing access token"; then
        print_success "WebSocket: Endpoint responding at ws://localhost${ws_path} (auth required)"
        print_verbose "Backend requires authentication (expected behavior)"
        return 0
    fi

    # Fallback: check HTTP status
    local status_code=$(curl -s -o /dev/null -w "%{http_code}" \
        -H "Connection: Upgrade" \
        -H "Upgrade: websocket" \
        http://localhost${ws_path} 2>/dev/null)

    if [[ "$status_code" == "200" ]]; then
        print_success "WebSocket: Endpoint responding at ws://localhost${ws_path}"
    elif [[ "$status_code" == "400" ]] || [[ "$status_code" == "401" ]] || [[ "$status_code" == "403" ]]; then
        print_success "WebSocket: Endpoint responding at ws://localhost${ws_path} (auth required)"
    elif [[ "$status_code" == "404" ]]; then
        print_error "WebSocket: Endpoint not found at ${ws_path}"
        if [[ "$ws_path" =~ "/ws/v2" ]]; then
            print_error "Note: WebSocket v2 requires format /ws/v2/{workspace_id}"
        fi
        return 1
    elif [[ "$status_code" == "502" ]] || [[ "$status_code" == "503" ]]; then
        print_error "WebSocket: Backend service unavailable (HTTP $status_code)"
        return 1
    else
        print_verbose "WebSocket: HTTP $status_code at ${ws_path}"
        print_success "WebSocket: Endpoint reachable (nginx routing configured)"
    fi

    return 0
}

check_admin_frontend() {
    print_verbose "Checking admin frontend..."

    # Try with trailing slash first (might redirect)
    local admin_response=$(curl -s -o /dev/null -w "%{http_code}" http://localhost/console/ 2>/dev/null)

    if [[ "$admin_response" == "200" ]]; then
        print_success "Admin Frontend: Accessible at http://localhost/console/"
    elif [[ "$admin_response" == "308" ]] || [[ "$admin_response" == "301" ]] || [[ "$admin_response" == "302" ]]; then
        # Redirects are OK - try following redirect
        local redirected_response=$(curl -s -L -o /dev/null -w "%{http_code}" http://localhost/console/ 2>/dev/null)
        if [[ "$redirected_response" == "200" ]]; then
            print_success "Admin Frontend: Accessible at http://localhost/console/ (redirect $admin_response)"
        else
            print_warning "Admin Frontend: Redirect loop or error (HTTP $admin_response -> $redirected_response)"
        fi
    elif [[ "$admin_response" == "404" ]]; then
        print_error "Admin Frontend: Not found (check ADMIN_FRONTEND_PATH_PREFIX in .env)"
        return 1
    elif [[ "$admin_response" == "502" ]] || [[ "$admin_response" == "503" ]]; then
        print_error "Admin Frontend: Service unavailable (HTTP $admin_response)"
        return 1
    else
        print_warning "Admin Frontend: Unexpected response (HTTP $admin_response)"
    fi

    return 0
}

check_collaboration_data() {
    print_verbose "Checking collaboration data..."

    local compose_cmd=$(get_compose_command)

    # Check if postgres container is running
    local pg_status=$($compose_cmd ps -q postgres 2>/dev/null)
    if [[ -z "$pg_status" ]]; then
        print_warning "PostgreSQL: Container not running"
        return 1
    fi

    # Check document count
    local collab_count=$($compose_cmd exec -T postgres psql -U postgres -d postgres -tAc "SELECT COUNT(*) FROM af_collab;" 2>/dev/null)
    if [[ $? -eq 0 ]] && [[ -n "$collab_count" ]] && [[ "$collab_count" -gt 0 ]]; then
        print_success "Documents: $collab_count documents in database"
    else
        print_info "Documents: No documents yet (fresh install)"
    fi

    # Check workspace count
    local workspace_count=$($compose_cmd exec -T postgres psql -U postgres -d postgres -tAc "SELECT COUNT(*) FROM af_workspace;" 2>/dev/null)
    if [[ $? -eq 0 ]] && [[ -n "$workspace_count" ]] && [[ "$workspace_count" -gt 0 ]]; then
        print_success "Workspaces: $workspace_count workspace(s) created"
    else
        print_info "Workspaces: No workspaces yet (fresh install)"
    fi

    # Check user count
    local user_count=$($compose_cmd exec -T postgres psql -U postgres -d postgres -tAc "SELECT COUNT(*) FROM af_user;" 2>/dev/null)
    if [[ $? -eq 0 ]] && [[ -n "$user_count" ]] && [[ "$user_count" -gt 0 ]]; then
        print_success "Users: $user_count user(s) registered"
    else
        print_info "Users: No users yet (fresh install)"
    fi

    # Check embeddings (for AI search features)
    local embedding_count=$($compose_cmd exec -T postgres psql -U postgres -d postgres -tAc "SELECT COUNT(*) FROM af_collab_embeddings;" 2>/dev/null)
    if [[ $? -eq 0 ]] && [[ -n "$embedding_count" ]]; then
        if [[ "$embedding_count" -gt 0 ]]; then
            print_success "AI Search: $embedding_count document embeddings indexed"
        else
            print_verbose "AI Search: No embeddings yet (indexing may be in progress)"
        fi
    fi

    return 0
}

check_ai_service() {
    print_verbose "Checking AI service integration..."

    local compose_cmd=$(get_compose_command)

    # Check if AI container is running
    local ai_status=$($compose_cmd ps -q ai 2>/dev/null)
    if [[ -z "$ai_status" ]]; then
        print_info "AI Service: Not running (optional feature)"
        return 0
    fi

    # Check AI service health
    local ai_health=$(curl -s -o /dev/null -w "%{http_code}" http://localhost:5001/health 2>/dev/null)
    if [[ "$ai_health" == "200" ]]; then
        print_success "AI Service: Healthy at http://localhost:5001"
    elif [[ "$ai_health" == "404" ]]; then
        # Try internal health check
        local ai_internal=$($compose_cmd exec -T ai sh -c 'curl -s http://localhost:5001/ 2>/dev/null' | head -20)
        if [[ -n "$ai_internal" ]]; then
            print_success "AI Service: Running (internal check passed)"
        else
            print_warning "AI Service: Health endpoint unavailable"
        fi
    else
        print_verbose "AI Service: HTTP $ai_health"
    fi

    return 0
}

check_published_features() {
    print_verbose "Checking publish/share features..."

    local compose_cmd=$(get_compose_command)

    # Check if postgres container is running
    local pg_status=$($compose_cmd ps -q postgres 2>/dev/null)
    if [[ -z "$pg_status" ]]; then
        return 1
    fi

    # Check published documents
    local published_count=$($compose_cmd exec -T postgres psql -U postgres -d postgres -tAc "SELECT COUNT(*) FROM af_published_collab;" 2>/dev/null)
    if [[ $? -eq 0 ]] && [[ -n "$published_count" ]]; then
        if [[ "$published_count" -gt 0 ]]; then
            print_success "Published Docs: $published_count document(s) published"
        else
            print_verbose "Published Docs: No published documents yet"
        fi
    fi

    # Check workspace invitations
    local invite_count=$($compose_cmd exec -T postgres psql -U postgres -d postgres -tAc "SELECT COUNT(*) FROM af_workspace_invitation;" 2>/dev/null)
    if [[ $? -eq 0 ]] && [[ -n "$invite_count" ]] && [[ "$invite_count" -gt 0 ]]; then
        print_verbose "Invitations: $invite_count pending invitation(s)"
    fi

    return 0
}

run_functional_tests() {
    [[ "$QUICK_MODE" == "true" ]] && return 0

    print_verbose "Running functional tests..."

    check_database_tables
    check_minio_storage
    check_api_endpoints
    check_websocket_endpoint
    check_admin_frontend
    check_collaboration_data
    check_ai_service
    check_published_features

    return 0
}

# ==================== LOG ANALYSIS ====================

check_admin_frontend_connectivity() {
    print_verbose "Checking Admin Frontend connectivity to AppFlowy Cloud..."

    local compose_cmd=$(get_compose_command)

    # Get admin_frontend container ID
    local container_id=$($compose_cmd ps -q admin_frontend 2>/dev/null)
    if [[ -z "$container_id" ]]; then
        print_verbose "Admin Frontend container not found"
        return 0
    fi

    if ! load_env_vars; then
        return 1
    fi

    # Admin Frontend connects to AppFlowy Cloud via APPFLOWY_BASE_URL
    # Analysis from docker logs and docker-compose.yml:
    # - admin_frontend gets: APPFLOWY_BASE_URL and APPFLOWY_GOTRUE_BASE_URL
    # - It makes API calls to APPFLOWY_BASE_URL (e.g., http://localhost)
    # - nginx proxy routes these to appflowy_cloud backend

    local base_url="${APPFLOWY_BASE_URL}"

    if [[ -z "$base_url" ]]; then
        print_error "Admin Frontend: APPFLOWY_BASE_URL not configured (CRITICAL)"
        print_error "  Admin Frontend cannot connect to AppFlowy Cloud without this"
        return 1
    fi

    # Check admin_frontend logs for connection errors
    local logs=$(docker logs --tail 200 "$container_id" 2>&1)

    # Look for connection errors in admin_frontend logs
    local connection_errors=$(echo "$logs" | grep -iE "error|ERROR|failed|ECONNREFUSED|ETIMEDOUT|fetch failed|network error|cannot connect" | grep -ivE "debug|verbose" | tail -15)

    if [[ -n "$connection_errors" ]]; then
        print_error "Admin Frontend: Connection errors detected:"
        echo ""

        local line_count=0
        while IFS= read -r line; do
            ((line_count++))
            local clean_line=$(echo "$line" | cut -c1-250)
            print_error "  → $clean_line"

            if [[ $line_count -ge 10 ]]; then
                print_error "  ... (showing first 10 errors)"
                break
            fi
        done <<< "$connection_errors"

        echo ""
        print_error "Common Admin Frontend login issues:"
        print_error "  1. APPFLOWY_BASE_URL ($base_url) not accessible from admin_frontend container"
        print_error "  2. Nginx not properly routing requests to appflowy_cloud backend"
        print_error "  3. AppFlowy Cloud service not running or crashed"
        print_error "  4. Network connectivity issues between containers"
        print_error ""
        print_error "Fix steps:"
        print_error "  1. Verify APPFLOWY_BASE_URL in .env matches your deployment"
        print_error "  2. Check nginx is running: docker compose ps nginx"
        print_error "  3. Check appflowy_cloud is running: docker compose ps appflowy_cloud"
        print_error "  4. Test from admin_frontend container:"
        print_error "     docker compose exec admin_frontend wget -O- $base_url/api/health"
        echo ""

        return 1
    else
        print_success "Admin Frontend: No connection errors detected"
        return 0
    fi
}

check_gotrue_auth_errors() {
    print_verbose "Checking GoTrue authentication logs..."

    local compose_cmd=$(get_compose_command)

    # Get GoTrue container ID
    local container_id=$($compose_cmd ps -q gotrue 2>/dev/null)
    if [[ -z "$container_id" ]]; then
        print_verbose "GoTrue container not found"
        return 0
    fi

    # Get last 300 lines of GoTrue logs and look for authentication errors
    # GoTrue logs are typically in text format with levels like ERROR, WARN, etc.
    local logs=$(docker logs --tail 300 "$container_id" 2>&1)

    # Extract error-level logs related to authentication/login
    local error_logs=$(echo "$logs" | grep -iE "error|ERROR|fatal|FATAL|panic|failed|invalid" | grep -ivE "dbug|debug" | tail -30)

    if [[ -n "$error_logs" ]]; then
        print_error "GoTrue: Authentication errors detected in logs:"
        echo ""

        local line_count=0
        local found_auth_issues=false

        while IFS= read -r line; do
            ((line_count++))

            # Check for common authentication error patterns
            if echo "$line" | grep -qiE "invalid.*credentials|authentication.*failed|invalid.*token|jwt.*invalid|password.*incorrect|user.*not.*found|login.*failed"; then
                found_auth_issues=true
                local clean_line=$(echo "$line" | cut -c1-250)
                print_error "  → $clean_line"
            elif echo "$line" | grep -qiE "error|ERROR|fatal|FATAL"; then
                # Show other errors too
                local clean_line=$(echo "$line" | cut -c1-250)
                print_error "  → $clean_line"
            fi

            # Limit output
            if [[ $line_count -ge 20 ]]; then
                print_error "  ... (showing first 20 errors, check full logs for more)"
                break
            fi
        done <<< "$error_logs"

        echo ""

        if [[ "$found_auth_issues" == "true" ]]; then
            print_error "Common login issue causes:"
            print_error "  1. Incorrect email/password"
            print_error "  2. JWT secret mismatch between GoTrue and AppFlowy Cloud"
            print_error "  3. User email not confirmed (check GOTRUE_MAILER_AUTOCONFIRM)"
            print_error "  4. OAuth configuration errors (if using Google/GitHub login)"
        fi

        print_error "  Full GoTrue logs: docker logs $container_id --tail 200"
        print_error "  Follow live: docker logs $container_id --follow"
        echo ""

        return 1
    else
        print_success "GoTrue: No authentication errors detected"
        return 0
    fi
}

check_container_errors() {
    print_verbose "Checking container crash logs and errors..."

    local compose_cmd=$(get_compose_command)
    local services="postgres redis gotrue appflowy_cloud admin_frontend nginx appflowy_worker ai"

    for service in $services; do
        # Check if service is defined in compose file
        if ! $compose_cmd config --services 2>/dev/null | grep -q "^${service}$"; then
            continue
        fi

        # Get container ID
        local container_id=$($compose_cmd ps -q $service 2>/dev/null)
        if [[ -z "$container_id" ]]; then
            continue
        fi

        # Get restart count and status
        local restart_count=$(docker inspect --format='{{.RestartCount}}' "$container_id" 2>/dev/null)
        local container_status=$(docker inspect --format='{{.State.Status}}' "$container_id" 2>/dev/null)

        # For GoTrue, always check logs even if running fine (login issues)
        # For other services, only check if there are restart issues
        local should_check_logs=false
        if [[ "$service" == "gotrue" ]]; then
            # Skip here, will be checked by check_gotrue_auth_errors
            continue
        elif [[ "$restart_count" -gt 3 ]] || [[ "$container_status" != "running" ]]; then
            should_check_logs=true
        fi

        if [[ "$should_check_logs" == "true" ]]; then
            print_warning "$service: Analyzing logs (restarts: $restart_count, status: $container_status)"

            # Get last 500 lines of logs to capture more context
            local logs=$(docker logs --tail 500 "$container_id" 2>&1)

            # Extract all error, warning, and fatal level logs
            # Support both JSON format and plain text logs
            local error_logs=$(echo "$logs" | grep -iE '"level":"(error|fatal|panic|warn)"|level=(error|fatal|panic|warn)|ERROR:|FATAL:|PANIC:|WARNING:|Failed to|Cannot|Error:|panic:' | tail -20)

            if [[ -n "$error_logs" ]]; then
                print_error "$service: Error/Warning logs detected:"
                echo ""

                local line_count=0
                while IFS= read -r line; do
                    ((line_count++))

                    # Try to parse JSON logs for better formatting
                    if echo "$line" | grep -q '"timestamp".*"level".*"message"'; then
                        local timestamp=$(echo "$line" | grep -oP '"timestamp":"[^"]*"' | cut -d'"' -f4 | cut -c1-19)
                        local level=$(echo "$line" | grep -oP '"level":"[^"]*"' | cut -d'"' -f4 | tr '[:lower:]' '[:upper:]')
                        local message=$(echo "$line" | grep -oP '"message":"[^"]*"' | cut -d'"' -f4)

                        if [[ -n "$timestamp" && -n "$level" && -n "$message" ]]; then
                            print_error "  [$timestamp] $level: $message"
                        else
                            # Fallback to showing raw line
                            local clean_line=$(echo "$line" | cut -c1-250)
                            print_error "  → $clean_line"
                        fi
                    else
                        # Plain text log - show with timestamp if available
                        local clean_line=$(echo "$line" | cut -c1-250)
                        print_error "  → $clean_line"
                    fi

                    # Limit output to prevent spam
                    if [[ $line_count -ge 15 ]]; then
                        print_error "  ... (showing first 15 errors, check full logs for more)"
                        break
                    fi
                done <<< "$error_logs"

                echo ""
                print_error "  Full logs: docker logs $container_id --tail 200"
                print_error "  Follow live: docker logs $container_id --follow"
                echo ""
            else
                # No errors found, check for recent activity
                local recent_logs=$(echo "$logs" | tail -5)
                if [[ -n "$recent_logs" ]]; then
                    print_info "$service: No errors found, recent activity:"
                    while IFS= read -r line; do
                        local clean_line=$(echo "$line" | cut -c1-200)
                        print_verbose "  $clean_line"
                    done <<< "$recent_logs"
                    echo ""
                fi
            fi
        fi
    done

    return 0
}

extract_container_crash_summary() {
    print_verbose "Generating container crash summary..."

    local compose_cmd=$(get_compose_command)
    local services="postgres redis gotrue appflowy_cloud admin_frontend nginx appflowy_worker ai"

    local has_crashes=false

    for service in $services; do
        if ! $compose_cmd config --services 2>/dev/null | grep -q "^${service}$"; then
            continue
        fi

        local container_id=$($compose_cmd ps -q $service 2>/dev/null)
        if [[ -z "$container_id" ]]; then
            continue
        fi

        local restart_count=$(docker inspect --format='{{.RestartCount}}' "$container_id" 2>/dev/null)
        local container_status=$(docker inspect --format='{{.State.Status}}' "$container_id" 2>/dev/null)
        local exit_code=$(docker inspect --format='{{.State.ExitCode}}' "$container_id" 2>/dev/null)

        # Report containers with issues
        if [[ "$restart_count" -gt 5 ]]; then
            has_crashes=true
            print_error "$service: High restart count ($restart_count times)"

            # Get the most recent fatal error
            local fatal_error=$(docker logs --tail 100 "$container_id" 2>&1 | grep -iE "fatal|panic|error" | tail -1)
            if [[ -n "$fatal_error" ]]; then
                local clean_error=$(echo "$fatal_error" | cut -c1-150)
                print_error "  Last error: $clean_error"
            fi

            # Provide fix suggestions
            print_error "  Fix: Check logs with: docker logs $container_id"
            echo ""
        elif [[ "$container_status" == "exited" ]]; then
            has_crashes=true
            print_error "$service: Container exited (exit code: $exit_code)"

            # Get exit reason
            local exit_msg=$(docker logs --tail 50 "$container_id" 2>&1 | tail -10)
            if [[ -n "$exit_msg" ]]; then
                print_error "  Recent logs:"
                while IFS= read -r line; do
                    local clean_line=$(echo "$line" | cut -c1-150)
                    print_error "    $clean_line"
                done <<< "$exit_msg"
            fi
            echo ""
        fi
    done

    if [[ "$has_crashes" == "false" ]]; then
        print_success "No container crashes detected - all services stable"
    fi

    return 0
}

analyze_service_logs() {
    [[ "$SKIP_LOGS" == "true" ]] && return 0

    print_verbose "Analyzing service logs..."

    local compose_cmd=$(get_compose_command)
    local services="gotrue appflowy_cloud nginx admin_frontend"

    # Critical error patterns (actual problems)
    local patterns=(
        "panic"
        "fatal"
        "FATAL"
        "connection refused"
        "cannot connect"
        "failed to start"
        "failed to connect"
        "\[error\]"
    )

    # Error level patterns (need context)
    local error_patterns=(
        '"level":"error"'
        '"level":"fatal"'
        'level=error'
        'level=fatal'
        'ERROR:'
        'FATAL:'
        '\[warn\]'
    )

    for service in $services; do
        print_verbose "Checking $service logs..."

        # Get last 100 lines of logs, exclude docker-compose warnings
        local logs=$($compose_cmd logs --tail=100 $service 2>&1 | grep -v '^time=".*level=warning')

        if [[ -z "$logs" ]]; then
            print_verbose "No logs available for $service"
            continue
        fi

        # Search for critical error patterns
        local found_errors=false
        for pattern in "${patterns[@]}"; do
            if echo "$logs" | grep -i "$pattern" &>/dev/null; then
                if [[ "$found_errors" == "false" ]]; then
                    print_error "$service: Found critical issues in logs"
                    found_errors=true
                fi

                if [[ "$INCLUDE_LOGS" == "true" ]]; then
                    local error_lines=$(echo "$logs" | grep -i "$pattern" | tail -3)
                    print_verbose "  Pattern '$pattern': $error_lines"
                fi
            fi
        done

        # Search for error level logs (less critical)
        if [[ "$found_errors" == "false" ]]; then
            for pattern in "${error_patterns[@]}"; do
                if echo "$logs" | grep "$pattern" &>/dev/null; then
                    print_warning "$service: Found error-level log entries"
                    found_errors=true

                    if [[ "$INCLUDE_LOGS" == "true" ]]; then
                        local error_lines=$(echo "$logs" | grep "$pattern" | tail -3)
                        print_verbose "  Error entries: $error_lines"
                    fi
                    break
                fi
            done
        fi

        if [[ "$found_errors" == "false" ]]; then
            print_success "$service: No errors in recent logs"
        fi
    done
}

# ==================== REPORT GENERATION ====================

generate_report() {
    local report_file="${OUTPUT_FILE:-$REPORT_FILE}"

    print_verbose "Generating report: $report_file"

    {
        echo "=========================================="
        echo "AppFlowy Cloud Diagnostic Report"
        echo "=========================================="
        echo "Generated: $(date)"
        echo "Version: $SCRIPT_VERSION"
        echo ""

        echo "=== ENVIRONMENT ==="
        echo "OS: $(uname -s)"
        echo "Docker: $(docker --version 2>/dev/null || echo 'Not installed')"
        echo "Compose: $(docker compose version 2>/dev/null || docker-compose --version 2>/dev/null || echo 'Not installed')"
        echo "Compose File: $COMPOSE_FILE"
        echo ""

        echo "=== SERVICE VERSIONS ==="
        if [[ -n "$APPFLOWY_CLOUD_VERSION" ]]; then
            echo "AppFlowy Cloud: $APPFLOWY_CLOUD_VERSION"
        else
            echo "AppFlowy Cloud: Unknown"
        fi
        if [[ -n "$ADMIN_FRONTEND_VERSION" ]]; then
            echo "Admin Frontend: $ADMIN_FRONTEND_VERSION"
        else
            echo "Admin Frontend: Unknown"
        fi
        if [[ -n "$DEPLOYMENT_TYPE" ]]; then
            echo "Deployment Type: $DEPLOYMENT_TYPE"
        fi
        echo ""

        if [[ ${#SUCCESSES[@]} -gt 0 ]]; then
            echo "=== SUCCESSES (${#SUCCESSES[@]}) ==="
            for success in "${SUCCESSES[@]}"; do
                echo "  ✓ $success"
            done
            echo ""
        fi

        if [[ ${#WARNINGS[@]} -gt 0 ]]; then
            echo "=== WARNINGS (${#WARNINGS[@]}) ==="
            for warning in "${WARNINGS[@]}"; do
                echo "  ⚠ $warning"
            done
            echo ""
        fi

        if [[ ${#ISSUES[@]} -gt 0 ]]; then
            echo "=== ISSUES (${#ISSUES[@]}) ==="
            for issue in "${ISSUES[@]}"; do
                echo "  ✗ $issue"
            done
            echo ""
        fi

        echo "=== RECOMMENDATIONS ==="
        generate_recommendations

    } > "$report_file"

    print_success "Report saved to: $report_file"
}

generate_recommendations() {
    local has_critical=false

    # Check for critical issues
    for issue in "${ISSUES[@]}"; do
        if echo "$issue" | grep -qi "jwt"; then
            echo "Priority 1 (CRITICAL): JWT Secret Mismatch"
            echo "  - Issue: JWT secrets between GoTrue and AppFlowy Cloud don't match"
            echo "  - Fix: Edit .env and ensure GOTRUE_JWT_SECRET equals APPFLOWY_GOTRUE_JWT_SECRET"
            echo "  - Then restart: docker compose down && docker compose up -d"
            echo ""
            has_critical=true
        fi

        if echo "$issue" | grep -qi "WebSocket.*wss://.*CRITICAL\|WebSocket URL is ws://"; then
            echo "Priority 1 (CRITICAL): WebSocket Scheme Mismatch"
            echo "  - Issue: Using HTTPS but WebSocket URL is ws:// instead of wss://"
            echo "  - Fix: Edit .env and change APPFLOWY_WS_BASE_URL to use wss://"
            echo "  - Example: APPFLOWY_WS_BASE_URL=wss://yourdomain.com/ws"
            echo "  - Then restart: docker compose restart appflowy_cloud nginx"
            echo ""
            has_critical=true
        fi

        if echo "$issue" | grep -qi "Nginx.*Missing.*WebSocket"; then
            echo "Priority 1 (CRITICAL): Nginx WebSocket Configuration Missing"
            echo "  - Issue: Nginx is not configured to proxy WebSocket connections"
            echo "  - Fix: Add WebSocket headers to nginx config:"
            echo "    location /ws {"
            echo "      proxy_pass http://appflowy_cloud:8000;"
            echo "      proxy_http_version 1.1;"
            echo "      proxy_set_header Upgrade \$http_upgrade;"
            echo "      proxy_set_header Connection \"upgrade\";"
            echo "      proxy_set_header Host \$host;"
            echo "    }"
            echo "  - Then: docker compose restart nginx"
            echo ""
            has_critical=true
        fi

        if echo "$issue" | grep -qi "SSL.*certificate"; then
            echo "Priority 1 (CRITICAL): SSL Certificate Issue"
            echo "  - Issue: SSL/TLS certificate is invalid, expired, or missing"
            echo "  - Fix: Install or renew SSL certificate"
            echo "  - For Let's Encrypt: certbot certonly --standalone -d yourdomain.com"
            echo "  - Update nginx config to point to certificate files"
            echo "  - Then: docker compose restart nginx"
            echo ""
            has_critical=true
        fi

        if echo "$issue" | grep -qi "not running\|exited\|restarting"; then
            echo "Priority 1 (CRITICAL): Service Not Running"
            echo "  - Issue: One or more critical services are not running"
            echo "  - Fix: Check logs with: docker compose logs [service_name]"
            echo "  - Restart: docker compose restart [service_name]"
            echo ""
            has_critical=true
        fi

        if echo "$issue" | grep -qi "database\|postgres"; then
            echo "Priority 1 (CRITICAL): Database Connection Issue"
            echo "  - Issue: Cannot connect to PostgreSQL"
            echo "  - Fix: Check DATABASE_URL in .env matches PostgreSQL credentials"
            echo "  - Verify: docker compose exec postgres pg_isready -U postgres"
            echo ""
            has_critical=true
        fi

        if echo "$issue" | grep -qi "SMTP Configuration Incomplete"; then
            echo "Priority 1 (CRITICAL): SMTP Configuration for Workspace Sharing"
            echo "  - Issue: GOTRUE_SMTP configured but APPFLOWY_MAILER_SMTP is missing"
            echo "  - Impact: Users CANNOT share workspaces or send invitations"
            echo "  - Common mistake: Configuring only GOTRUE_SMTP (for auth) but forgetting APPFLOWY_MAILER_SMTP (for invitations)"
            echo "  - Fix: Both SMTP configurations are required:"
            echo "    - GOTRUE_SMTP_* for authentication emails (signup, password reset)"
            echo "    - APPFLOWY_MAILER_SMTP_* for workspace invitations and sharing"
            echo "  - Add to .env and restart: docker compose restart appflowy_cloud"
            echo ""
            has_critical=true
        fi

        if echo "$issue" | grep -qi "High restart count"; then
            echo "Priority 1 (CRITICAL): Container Crash Loop Detected"
            echo "  - Issue: Container is repeatedly crashing and restarting"
            echo "  - Common causes:"
            echo "    - Configuration errors in .env file"
            echo "    - Database connection failures"
            echo "    - Missing or incorrect credentials"
            echo "    - Port conflicts with other services"
            echo "  - Fix:"
            echo "    1. Check container logs: docker logs <container_id>"
            echo "    2. Look for error messages at container startup"
            echo "    3. Verify .env configuration matches requirements"
            echo "    4. Ensure all dependent services (postgres, redis) are running"
            echo ""
            has_critical=true
        fi

        if echo "$issue" | grep -qi "Container exited"; then
            echo "Priority 1 (CRITICAL): Container Failed to Start"
            echo "  - Issue: Container exited and is not running"
            echo "  - Fix:"
            echo "    1. Check exit logs above for specific error"
            echo "    2. Verify configuration in .env file"
            echo "    3. Try: docker compose up <service_name> --force-recreate"
            echo "    4. If persists, check: docker compose logs <service_name>"
            echo ""
            has_critical=true
        fi
    done

    # Check for warnings
    for warning in "${WARNINGS[@]}"; do
        if echo "$warning" | grep -qi "restart"; then
            echo "Priority 2 (Important): High Restart Count"
            echo "  - Issue: Service is restarting frequently"
            echo "  - Fix: Check service logs for crash reasons"
            echo "  - Command: docker compose logs --tail=200 [service_name]"
            echo ""
        fi
    done

    if [[ "$has_critical" == "false" && ${#ISSUES[@]} -eq 0 ]]; then
        echo "✓ No critical issues detected"
        echo ""
        echo "If you're still experiencing login or WebSocket problems, check:"
        echo "  1. Browser console for JavaScript errors"
        echo "  2. Network tab for failed API or WebSocket requests"
        echo "  3. Ensure you're using the correct admin credentials"
        echo "  4. Try clearing browser cache/cookies"
        echo "  5. Check browser WebSocket connection in DevTools (Network > WS)"
    fi
}

# ==================== MAIN EXECUTION ====================

main() {
    parse_arguments "$@"

    # Print header
    if [[ "$JSON_OUTPUT" != "true" ]]; then
        print_header "AppFlowy Cloud Diagnostic Tool v${SCRIPT_VERSION}"
        echo ""
    fi

    # Phase 1: Environment Check
    if [[ "$QUIET" != "true" ]]; then
        print_header "Phase 1: Environment Check"
    fi

    check_docker || exit 1
    check_docker_compose > /dev/null || exit 1
    detect_compose_file || exit 1
    check_env_file || exit 1

    echo ""

    # Phase 2: Container Status
    if [[ "$QUIET" != "true" ]]; then
        print_header "Phase 2: Container Status"
    fi

    check_container_status
    check_service_versions

    echo ""

    # Phase 3: Configuration Validation
    if [[ "$QUIET" != "true" ]]; then
        print_header "Phase 3: Configuration Validation"
    fi

    check_duplicate_env_keys
    check_legacy_env_vars
    check_jwt_secrets
    check_database_urls
    check_base_urls
    check_scheme_consistency
    check_gotrue_configuration
    check_admin_credentials
    check_smtp_configuration
    check_nginx_websocket_config
    check_production_https_websocket
    check_ssl_certificate
    check_websocket_cors_headers

    echo ""

    # Phase 4: Health Checks
    if [[ "$QUICK_MODE" != "true" ]]; then
        if [[ "$QUIET" != "true" ]]; then
            print_header "Phase 4: Health Checks"
        fi

        check_health_endpoints

        echo ""
    fi

    # Phase 5: Functional Tests
    if [[ "$QUICK_MODE" != "true" ]]; then
        if [[ "$QUIET" != "true" ]]; then
            print_header "Phase 5: Functional Tests"
        fi

        run_functional_tests

        echo ""
    fi

    # Phase 6: Log Analysis
    if [[ "$SKIP_LOGS" != "true" ]]; then
        if [[ "$QUIET" != "true" ]]; then
            print_header "Phase 6: Log Analysis"
        fi

        check_admin_frontend_connectivity
        check_gotrue_auth_errors
        check_container_errors
        extract_container_crash_summary
        analyze_service_logs

        echo ""
    fi

    # Summary
    if [[ "$QUIET" != "true" ]]; then
        print_header "Summary"
        echo ""
        echo "Successes: ${#SUCCESSES[@]}"
        echo "Warnings:  ${#WARNINGS[@]}"
        echo "Issues:    ${#ISSUES[@]}"
        echo ""

        if [[ ${#ISSUES[@]} -gt 0 ]]; then
            print_header "Recommendations"
            generate_recommendations
            echo ""
        fi
    fi

    # Generate report
    generate_report

    # Exit code based on issues
    if [[ ${#ISSUES[@]} -gt 0 ]]; then
        exit 1
    else
        exit 0
    fi
}

# Run main function
main "$@"
