#!/bin/bash

# =============================================================================
# AppFlowy Cloud - Diagnostic Tool - Health Checks
# =============================================================================
# This module handles service health endpoint checks including:
# - HTTP health endpoint verification
# - Service-specific health checks (GoTrue, AppFlowy Cloud)
# - Database connectivity checks (PostgreSQL, Redis)
# - Development vs Production mode detection
# =============================================================================

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
