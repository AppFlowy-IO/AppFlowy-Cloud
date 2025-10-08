#!/bin/bash

# =============================================================================
# AppFlowy Cloud - Diagnostic Tool - Container Checks
# =============================================================================
# This module handles Docker and container-related checks including:
# - Docker installation verification
# - Docker Compose detection
# - Container status monitoring
# - Service version detection
# =============================================================================

# ==================== DOCKER ENVIRONMENT ====================

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

# ==================== CONTAINER STATUS ====================

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
