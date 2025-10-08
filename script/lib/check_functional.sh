#!/bin/bash

# =============================================================================
# AppFlowy Cloud - Diagnostic Tool - Functional Tests
# =============================================================================
# This module handles functional testing of AppFlowy Cloud components including:
# - Storage verification (Minio/S3)
# - Database schema validation
# - API endpoint testing
# - WebSocket connectivity checks
# - Admin frontend accessibility
# - Collaboration data verification
# - AI service integration
# - Published features validation
# - WebSocket connection simulation for login hang detection
# =============================================================================

# ==================== STORAGE CHECKS ====================

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

# ==================== DATABASE CHECKS ====================

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

# ==================== API ENDPOINT CHECKS ====================

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

# ==================== WEBSOCKET CHECKS ====================

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

check_websocket_connection_simulation() {
    print_verbose "Simulating WebSocket connection for login hang detection..."

    if ! load_env_vars; then
        print_warning "WebSocket Simulation: Cannot load .env configuration"
        return 0
    fi

    local base_url="${APPFLOWY_BASE_URL}"
    local ws_url="${APPFLOWY_WS_BASE_URL:-${APPFLOWY_WEBSOCKET_BASE_URL}}"

    if [[ -z "$ws_url" ]]; then
        print_warning "WebSocket Simulation: WS URL not configured, skipping connection test"
        return 0
    fi

    # Extract scheme and check for common misconfigurations
    local ws_scheme=$(extract_url_scheme "$ws_url")
    local base_scheme=$(extract_url_scheme "$base_url")

    # Critical: Check for HTTPS + WS (not WSS) mismatch
    if [[ "$base_scheme" == "https" && "$ws_scheme" == "ws" ]]; then
        print_error "WebSocket Connection: CRITICAL MISCONFIGURATION DETECTED"
        print_error "  Base URL: $base_url (HTTPS)"
        print_error "  WebSocket URL: $ws_url (WS)"
        print_error ""
        print_error "  ⚠️  This WILL cause login to hang at 100%!"
        print_error "  Browsers block insecure WebSocket (ws://) from HTTPS pages"
        print_error ""
        print_error "  Fix: Change WS_SCHEME=wss in .env file"
        print_error "  Then: docker compose up -d"
        echo ""
        return 1
    fi

    # Test WebSocket endpoint availability (without auth)
    local ws_test_url
    if [[ "$ws_url" == *"/ws/v2" ]]; then
        # For v2, test with a dummy workspace ID
        ws_test_url="http://localhost/ws/v2/00000000-0000-0000-0000-000000000000"
    else
        # For v1 or other versions
        ws_test_url="http://localhost${ws_url##*${base_url}}"
    fi

    # Try WebSocket upgrade request
    local response=$(curl -s -o /dev/null -w "%{http_code}" \
        -X GET "$ws_test_url" \
        -H "Upgrade: websocket" \
        -H "Connection: Upgrade" \
        -H "Sec-WebSocket-Version: 13" \
        -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
        --max-time 5 2>/dev/null)

    if [[ "$response" == "401" || "$response" == "403" ]]; then
        print_success "WebSocket Connection: Endpoint reachable (auth required - expected)"
        print_verbose "  WebSocket upgrade request returned: $response"
    elif [[ "$response" == "404" ]]; then
        print_error "WebSocket Connection: Endpoint not found (404)"
        print_error "  This will cause login to hang after authentication"
        print_error "  Fix: Verify nginx WebSocket routing configuration"
        echo ""
        return 1
    elif [[ "$response" == "502" || "$response" == "503" ]]; then
        print_error "WebSocket Connection: Backend unavailable ($response)"
        print_error "  AppFlowy Cloud service may not be running properly"
        print_error "  Check: docker compose ps appflowy_cloud"
        echo ""
        return 1
    elif [[ "$response" == "000" ]]; then
        print_warning "WebSocket Connection: Connection timeout or refused"
        print_warning "  This may indicate nginx or network issues"
    else
        print_verbose "WebSocket Connection: Received HTTP $response"
    fi

    # Additional check: verify nginx WebSocket proxy configuration exists
    local compose_cmd=$(get_compose_command)
    local nginx_container=$($compose_cmd ps -q nginx 2>/dev/null)

    if [[ -n "$nginx_container" ]]; then
        local nginx_config=$(docker exec "$nginx_container" cat /etc/nginx/nginx.conf 2>/dev/null || echo "")

        if [[ -n "$nginx_config" ]]; then
            # Check for WebSocket upgrade headers
            if ! echo "$nginx_config" | grep -q "Upgrade.*\$http_upgrade"; then
                print_warning "WebSocket Connection: Nginx may be missing WebSocket upgrade headers"
                print_warning "  This can cause connection failures after login"
            fi
        fi
    fi

    return 0
}

# ==================== ADMIN FRONTEND CHECKS ====================

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

# ==================== COLLABORATION DATA CHECKS ====================

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

# ==================== AI SERVICE CHECKS ====================

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

# ==================== PUBLISHED FEATURES CHECKS ====================

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

# ==================== FUNCTIONAL TEST SUITE ====================

run_functional_tests() {
    [[ "$QUICK_MODE" == "true" ]] && return 0

    print_verbose "Running functional tests..."

    check_database_tables
    check_minio_storage
    check_api_endpoints
    check_websocket_endpoint
    check_websocket_connection_simulation
    check_admin_frontend
    check_collaboration_data
    check_ai_service
    check_published_features

    return 0
}
