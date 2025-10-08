#!/bin/bash

# =============================================================================
# AppFlowy Cloud - Diagnostic Tool - Configuration Validation
# =============================================================================
# This module handles configuration validation including:
# - URL scheme alignment and consistency checks
# - WebSocket configuration validation
# - Environment variable duplicate detection
# - Legacy environment variable detection
# - GoTrue service configuration
# - JWT secret validation
# - Database URL validation
# - Base URL configuration
# - Admin credentials verification
# - SMTP configuration validation
# - AI server configuration
# - Nginx WebSocket configuration
# - SSL/TLS certificate verification
# - Production HTTPS/WSS validation
# - WebSocket CORS and security headers
# - Plan limits validation
# - User authentication flow validation
# =============================================================================

# ==================== URL VALIDATION ====================

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

# ==================== ENVIRONMENT VARIABLE VALIDATION ====================

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

# ==================== SERVICE CONFIGURATION VALIDATION ====================

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

check_ai_server_configuration() {
    print_verbose "Checking AI server configuration..."

    if ! load_env_vars; then
        return 1
    fi

    local ai_server_host="${AI_SERVER_HOST}"
    local ai_server_port="${AI_SERVER_PORT}"
    local appflowy_ai_version="${APPFLOWY_AI_VERSION}"

    # Check if AI server is configured
    if [[ -z "$ai_server_host" && -z "$ai_server_port" ]]; then
        print_info "AI Server: Not configured (AI features disabled)"
        print_info "  Without AI server, the following features won't work:"
        print_info "    - AI-powered chat and assistance"
        print_info "    - Document AI completions"
        print_info "    - Smart search and indexing"
        print_info "  To enable AI features, add to .env file:"
        print_info "    AI_SERVER_HOST=ai  # or 'localhost' for local development"
        print_info "    AI_SERVER_PORT=5001"
        print_info "    APPFLOWY_AI_VERSION=latest  # or specific version"
        return 0
    fi

    # Default values if only partially configured
    ai_server_host="${ai_server_host:-localhost}"
    ai_server_port="${ai_server_port:-5001}"

    print_success "AI Server configured: $ai_server_host:$ai_server_port"
    print_verbose "  Used for: AI chat, completions, embeddings, and smart search"

    # Check AI version if specified
    if [[ -n "$appflowy_ai_version" ]]; then
        print_verbose "  AI Server Version: $appflowy_ai_version"
    else
        print_verbose "  AI Server Version: latest (default)"
    fi

    # Check if AI container is running (if using docker-compose)
    local compose_cmd=$(get_compose_command 2>/dev/null)
    if [[ -n "$compose_cmd" ]]; then
        # Check if 'ai' service is defined in compose file
        if $compose_cmd config --services 2>/dev/null | grep -q "^ai$"; then
            local ai_container=$($compose_cmd ps -q ai 2>/dev/null)
            if [[ -n "$ai_container" ]]; then
                local container_status=$(docker inspect --format='{{.State.Status}}' $ai_container 2>/dev/null)
                if [[ "$container_status" == "running" ]]; then
                    print_success "AI container: Running"
                else
                    print_warning "AI container exists but is not running (status: $container_status)"
                    print_warning "  Fix: docker compose restart ai"
                fi
            else
                print_warning "AI service defined but container not found"
                print_warning "  Fix: docker compose up -d ai"
            fi
        else
            print_verbose "AI service not defined in docker-compose.yml (may be external)"
        fi
    fi

    # Validate host/port make sense
    if [[ "$ai_server_host" == "localhost" || "$ai_server_host" == "127.0.0.1" ]]; then
        print_verbose "  AI server configured for local development"
    elif [[ "$ai_server_host" == "ai" ]]; then
        print_verbose "  AI server configured for docker-compose deployment"
    else
        print_info "  AI server configured for external deployment: $ai_server_host"
    fi

    return 0
}

# ==================== NGINX CONFIGURATION VALIDATION ====================

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

# ==================== PLAN AND USER VALIDATION ====================

check_plan_limits() {
    print_verbose "Checking self-hosted plan limits..."

    local compose_cmd=$(get_compose_command)

    # Get appflowy_cloud container ID
    local container_id=$($compose_cmd ps -q appflowy_cloud 2>/dev/null)
    if [[ -z "$container_id" ]]; then
        print_verbose "AppFlowy Cloud container not found"
        return 0
    fi

    # Parse logs for "Free plan limits" message
    local logs=$(docker logs --tail 200 "$container_id" 2>&1)
    local plan_limits=$(echo "$logs" | grep -i "Free plan limits" | tail -1)

    if [[ -n "$plan_limits" ]]; then
        # Extract max_users and max_guests from log line (BSD grep compatible)
        local max_users=$(echo "$plan_limits" | sed -n 's/.*max_users:[[:space:]]*\([0-9][0-9]*\).*/\1/p')
        local max_guests=$(echo "$plan_limits" | sed -n 's/.*max_guests:[[:space:]]*\([0-9][0-9]*\).*/\1/p')

        # Fallback to "unknown" if extraction failed
        [[ -z "$max_users" ]] && max_users="unknown"
        [[ -z "$max_guests" ]] && max_guests="unknown"

        if [[ "$max_users" != "unknown" && "$max_guests" != "unknown" ]]; then
            print_info "Self-Hosted Plan Configuration:"
            print_info "  Max Users: $max_users"
            print_info "  Max Guests: $max_guests"

            if [[ "$max_users" == "1" ]]; then
                print_info "  Note: Self-hosted version configured with 1-user limit"
                print_info "        Admin and user can use different emails"
            fi

            # Try to count existing users (if database accessible)
            if $compose_cmd ps postgres &>/dev/null; then
                local user_count=$($compose_cmd exec -T postgres psql -U postgres -d postgres -tAc "SELECT COUNT(*) FROM af_user WHERE deleted_at IS NULL;" 2>/dev/null || echo "unknown")

                if [[ "$user_count" != "unknown" && "$user_count" =~ ^[0-9]+$ ]]; then
                    if [[ "$max_users" != "unknown" && "$max_users" =~ ^[0-9]+$ ]]; then
                        if [[ $user_count -ge $max_users ]]; then
                            print_warning "User limit reached! ($user_count/$max_users users)"
                            print_warning "  - New users will not be able to log in"
                            print_warning "  - Consider removing unused accounts to free up slots"
                        else
                            print_success "Users: $user_count/$max_users (within limit)"
                        fi
                    else
                        print_info "  Current user count: $user_count"
                    fi
                fi
            fi
            echo ""
        else
            print_verbose "Could not parse plan limits from logs"
        fi
    else
        print_verbose "No plan limits found in logs (container may be using custom configuration)"
    fi

    return 0
}

check_user_auth_flow() {
    print_verbose "Validating user authentication flow..."

    if ! load_env_vars; then
        return 0
    fi

    local compose_cmd=$(get_compose_command)

    # Check if GoTrue is configured for auto-confirm (important for testing)
    local mailer_autoconfirm="${GOTRUE_MAILER_AUTOCONFIRM:-false}"

    if [[ "$mailer_autoconfirm" == "false" ]]; then
        print_warning "User Auth: Email confirmation required (GOTRUE_MAILER_AUTOCONFIRM=false)"
        print_warning "  - New users must confirm email before login"
        print_warning "  - Check SMTP settings if emails not arriving"
        print_warning "  - For testing, set GOTRUE_MAILER_AUTOCONFIRM=true"
        echo ""
    else
        print_success "User Auth: Auto-confirm enabled (good for testing)"
    fi

    # Check for OTP configuration
    local enable_otp="${GOTRUE_EXTERNAL_PHONE_ENABLED:-false}"
    if [[ "$enable_otp" == "true" ]]; then
        print_info "User Auth: OTP/Phone authentication enabled"
    fi

    # Verify GoTrue is accessible
    local gotrue_health=$(curl -s -o /dev/null -w "%{http_code}" "http://localhost/gotrue/health" --max-time 3 2>/dev/null)

    if [[ "$gotrue_health" == "200" ]]; then
        print_success "User Auth: GoTrue service healthy"
    elif [[ "$gotrue_health" == "404" ]]; then
        print_error "User Auth: GoTrue health endpoint not found"
        print_error "  This suggests nginx routing issues"
        print_error "  Fix: Verify nginx.conf has /gotrue/* proxy configuration"
        echo ""
    else
        print_warning "User Auth: GoTrue health check returned: $gotrue_health"
    fi

    # Check for common auth flow issues in logs
    local gotrue_container=$($compose_cmd ps -q gotrue 2>/dev/null)
    if [[ -n "$gotrue_container" ]]; then
        local recent_logs=$(docker logs --tail 100 "$gotrue_container" 2>&1)

        # Look for email delivery issues
        if echo "$recent_logs" | grep -qiE "failed to send.*email|smtp.*error|mailer.*failed"; then
            print_error "User Auth: Email delivery issues detected in GoTrue logs"
            print_error "  Users may not receive confirmation emails"
            print_error "  Check SMTP configuration in .env"
            echo ""
        fi

        # Look for token/JWT issues
        if echo "$recent_logs" | grep -qiE "invalid.*token|jwt.*error|signature.*invalid"; then
            print_error "User Auth: JWT/Token validation issues detected"
            print_error "  Verify GOTRUE_JWT_SECRET matches across services"
            echo ""
        fi
    fi

    return 0
}
