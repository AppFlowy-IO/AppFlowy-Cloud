#!/bin/bash

# =============================================================================
# AppFlowy Cloud - Diagnostic Tool - Log Analysis
# =============================================================================
# This module handles log analysis and error detection including:
# - Admin Frontend connectivity and error checks
# - GoTrue authentication error analysis
# - Container error and crash detection
# - Service log analysis for common issues
# =============================================================================

# ==================== ADMIN FRONTEND LOGS ====================

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

    # Check admin_frontend logs for connection errors (last 5 minutes only)
    local logs=$(docker logs --since 5m "$container_id" 2>&1)

    # Find the last version number (indicates container restart)
    # Only analyze logs after the last version number
    local last_version_line=$(echo "$logs" | grep -n "^Version:" | tail -1 | cut -d: -f1)
    if [[ -n "$last_version_line" ]]; then
        logs=$(echo "$logs" | tail -n +$last_version_line)
    fi

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

check_admin_frontend_errors() {
    print_verbose "Checking Admin Frontend specific errors..."

    local compose_cmd=$(get_compose_command)

    # Get admin_frontend container ID
    local container_id=$($compose_cmd ps -q admin_frontend 2>/dev/null)
    if [[ -z "$container_id" ]]; then
        print_verbose "Admin Frontend container not found"
        return 0
    fi

    # Check container status
    local container_status=$(docker inspect --format='{{.State.Status}}' "$container_id" 2>/dev/null)
    local restart_count=$(docker inspect --format='{{.RestartCount}}' "$container_id" 2>/dev/null)

    # Get logs (last 5 minutes only)
    local logs=$(docker logs --since 5m "$container_id" 2>&1)

    # Find the last version number (indicates container restart)
    # Only analyze logs after the last version number
    local last_version_line=$(echo "$logs" | grep -n "^Version:" | tail -1 | cut -d: -f1)
    if [[ -n "$last_version_line" ]]; then
        logs=$(echo "$logs" | tail -n +$last_version_line)
    fi

    # Check for specific error patterns
    local has_errors=false

    # Pattern 1: EISDIR error (node_modules issue)
    if echo "$logs" | grep -q "EISDIR.*node_modules"; then
        has_errors=true
        print_error "Admin Frontend: EISDIR error detected (node_modules issue)"
        local error_line=$(echo "$logs" | grep "EISDIR.*node_modules" | tail -1 | cut -c1-200)
        print_error "  Error: $error_line"
        print_error "  Cause: Likely a Docker volume or build cache issue"
        print_error "  Fix: Try rebuilding the admin_frontend image:"
        print_error "    docker compose build --no-cache admin_frontend"
        print_error "    docker compose up -d admin_frontend"
        echo ""
    fi

    # Pattern 2: Module resolution errors
    if echo "$logs" | grep -qiE "Cannot find module|Module not found|Error: Cannot resolve"; then
        has_errors=true
        print_error "Admin Frontend: Module resolution error detected"
        local error_line=$(echo "$logs" | grep -iE "Cannot find module|Module not found|Error: Cannot resolve" | tail -1 | cut -c1-200)
        print_error "  Error: $error_line"
        print_error "  Fix: Rebuild with fresh dependencies:"
        print_error "    docker compose build --no-cache admin_frontend"
        echo ""
    fi

    # Pattern 3: Configuration injection failures
    if echo "$logs" | grep -qiE "Failed to inject configuration|Configuration error|APPFLOWY_BASE_URL.*undefined"; then
        has_errors=true
        print_error "Admin Frontend: Configuration injection failed"
        local error_line=$(echo "$logs" | grep -iE "Failed to inject configuration|Configuration error" | tail -1 | cut -c1-200)
        print_error "  Error: $error_line"
        print_error "  Fix: Verify .env file has correct APPFLOWY_BASE_URL"
        print_error "    Check: APPFLOWY_BASE_URL is set and matches your deployment URL"
        echo ""
    fi

    # Pattern 4: Port binding errors
    if echo "$logs" | grep -qiE "EADDRINUSE.*3000|port.*already in use"; then
        has_errors=true
        print_error "Admin Frontend: Port conflict detected"
        print_error "  Error: Port 3000 already in use"
        print_error "  Fix: Another service is using the port"
        print_error "    Check: docker ps | grep 3000"
        echo ""
    fi

    # Pattern 5: Bun/Node runtime errors
    if echo "$logs" | grep -qiE "bun.*error|node.*fatal|v8::"; then
        has_errors=true
        local error_line=$(echo "$logs" | grep -iE "bun.*error|node.*fatal|v8::" | tail -1 | cut -c1-200)
        print_error "Admin Frontend: Runtime error"
        print_error "  Error: $error_line"
        echo ""
    fi

    # Check if container is constantly restarting
    if [[ "$restart_count" -gt 3 ]]; then
        print_error "Admin Frontend: Container restarting frequently (count: $restart_count)"
        print_error "  Status: $container_status"

        # Get the last startup attempt
        local startup_logs=$(echo "$logs" | tail -50)
        local last_error=$(echo "$startup_logs" | grep -iE "error|fatal|failed" | tail -3)

        if [[ -n "$last_error" ]]; then
            print_error "  Recent errors:"
            while IFS= read -r line; do
                local clean_line=$(echo "$line" | cut -c1-200)
                print_error "    $clean_line"
            done <<< "$last_error"
        fi
        echo ""
        has_errors=true
    fi

    if [[ "$has_errors" == "false" && "$container_status" == "running" ]]; then
        print_success "Admin Frontend: No specific startup errors detected"
    fi

    return 0
}

# ==================== GOTRUE AUTHENTICATION ====================

check_gotrue_auth_errors() {
    print_verbose "Checking GoTrue authentication logs..."

    local compose_cmd=$(get_compose_command)

    # Get GoTrue container ID
    local container_id=$($compose_cmd ps -q gotrue 2>/dev/null)
    if [[ -z "$container_id" ]]; then
        print_verbose "GoTrue container not found"
        return 0
    fi

    # Get GoTrue logs from last 5 minutes and look for authentication errors
    # GoTrue logs are typically in text format with levels like ERROR, WARN, etc.
    local logs=$(docker logs --since 5m "$container_id" 2>&1)

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

# ==================== CONTAINER ERROR ANALYSIS ====================

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

            # Get logs from last 10 minutes to capture recent context
            local logs=$(docker logs --since 10m "$container_id" 2>&1)

            # Find the last version/startup marker (indicates container restart)
            # Only analyze logs after the last restart
            if [[ "$service" == "appflowy_cloud" ]]; then
                local last_version_line=$(echo "$logs" | grep -n "Using AppFlowy Cloud version:" | tail -1 | cut -d: -f1)
                if [[ -n "$last_version_line" ]]; then
                    logs=$(echo "$logs" | tail -n +$last_version_line)
                fi
            elif [[ "$service" == "admin_frontend" ]]; then
                local last_version_line=$(echo "$logs" | grep -n "^Version:" | tail -1 | cut -d: -f1)
                if [[ -n "$last_version_line" ]]; then
                    logs=$(echo "$logs" | tail -n +$last_version_line)
                fi
            fi

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
                        local timestamp=$(echo "$line" | sed -n 's/.*"timestamp":"\([^"]*\)".*/\1/p' | cut -c1-19)
                        local level=$(echo "$line" | sed -n 's/.*"level":"\([^"]*\)".*/\1/p' | tr '[:lower:]' '[:upper:]')
                        local message=$(echo "$line" | sed -n 's/.*"message":"\([^"]*\)".*/\1/p')

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

            # Get the most recent fatal error from last 10 minutes
            local fatal_error=$(docker logs --since 10m "$container_id" 2>&1 | grep -iE "fatal|panic|error" | tail -1)
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

            # Get exit reason from recent logs
            local exit_msg=$(docker logs --since 10m "$container_id" 2>&1 | tail -10)
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

# ==================== SERVICE LOG ANALYSIS ====================

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

        # Get logs from last 5 minutes only, exclude docker-compose warnings
        local logs=$($compose_cmd logs --since=5m $service 2>&1 | grep -v '^time=".*level=warning')

        if [[ -z "$logs" ]]; then
            print_verbose "No logs available for $service"
            continue
        fi

        # Find the last version/startup marker (indicates container restart)
        # Only analyze logs after the last restart
        if [[ "$service" == "appflowy_cloud" ]]; then
            local last_version_line=$(echo "$logs" | grep -n "Using AppFlowy Cloud version:" | tail -1 | cut -d: -f1)
            if [[ -n "$last_version_line" ]]; then
                logs=$(echo "$logs" | tail -n +$last_version_line)
            fi
        elif [[ "$service" == "admin_frontend" ]]; then
            local last_version_line=$(echo "$logs" | grep -n "^admin_frontend.*| Version:" | tail -1 | cut -d: -f1)
            if [[ -n "$last_version_line" ]]; then
                logs=$(echo "$logs" | tail -n +$last_version_line)
            fi
        fi

        # Search for critical error patterns
        local found_errors=false
        for pattern in "${patterns[@]}"; do
            if echo "$logs" | grep -i "$pattern" &>/dev/null; then
                if [[ "$found_errors" == "false" ]]; then
                    print_error "$service: Found critical issues in logs"
                    found_errors=true
                fi

                # Always show critical errors (not just when INCLUDE_LOGS=true)
                local error_lines=$(echo "$logs" | grep -i "$pattern" | tail -3)
                if [[ -n "$error_lines" ]]; then
                    while IFS= read -r line; do
                        local clean_line=$(echo "$line" | cut -c1-200)
                        print_error "  → $clean_line"
                    done <<< "$error_lines"
                fi
            fi
        done

        # Search for error level logs (less critical)
        if [[ "$found_errors" == "false" ]]; then
            for pattern in "${error_patterns[@]}"; do
                if echo "$logs" | grep "$pattern" &>/dev/null; then
                    print_warning "$service: Found error-level log entries"
                    found_errors=true

                    # Always show error entries (not just when INCLUDE_LOGS=true)
                    local error_lines=$(echo "$logs" | grep "$pattern" | tail -3)
                    if [[ -n "$error_lines" ]]; then
                        while IFS= read -r line; do
                            local clean_line=$(echo "$line" | cut -c1-200)
                            print_warning "  → $clean_line"
                        done <<< "$error_lines"
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
