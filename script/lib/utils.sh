#!/bin/bash

# =============================================================================
# AppFlowy Cloud - Diagnostic Tool - Utility Functions
# =============================================================================
# This module provides utility functions for output formatting, color support,
# environment variable loading, and URL parsing.
# =============================================================================

# ==================== OUTPUT FUNCTIONS ====================

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

# ==================== DATA HANDLING ====================

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

# ==================== ENVIRONMENT LOADING ====================

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

# ==================== URL PARSING ====================

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

# ==================== HELP ====================

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
