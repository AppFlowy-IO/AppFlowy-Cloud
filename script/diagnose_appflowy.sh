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
SCRIPT_VERSION="2.0.0"
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

# ==================== LOAD MODULES ====================

# Load utility functions
source "$SCRIPT_DIR/lib/utils.sh"

# Load check modules
source "$SCRIPT_DIR/lib/check_containers.sh"
source "$SCRIPT_DIR/lib/check_config.sh"
source "$SCRIPT_DIR/lib/check_health.sh"
source "$SCRIPT_DIR/lib/check_functional.sh"
source "$SCRIPT_DIR/lib/check_logs.sh"
source "$SCRIPT_DIR/lib/report.sh"

# ==================== MAIN FUNCTION ====================

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
    check_user_auth_flow
    check_admin_credentials
    check_smtp_configuration
    check_ai_server_configuration
    check_nginx_websocket_config
    check_production_https_websocket
    check_ssl_certificate
    check_websocket_cors_headers
    check_plan_limits

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
        check_admin_frontend_errors
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
