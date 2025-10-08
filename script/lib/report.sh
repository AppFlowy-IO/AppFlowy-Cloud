#!/bin/bash

# =============================================================================
# AppFlowy Cloud - Diagnostic Tool - Report Generation
# =============================================================================
# This module handles report generation and recommendation output including:
# - Generating diagnostic reports with environment, version, and check results
# - Analyzing issues and warnings to generate actionable recommendations
# - Providing prioritized fix instructions for common problems
# =============================================================================

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
