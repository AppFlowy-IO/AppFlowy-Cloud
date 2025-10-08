# AppFlowy Cloud Diagnostic Tool - Modular Architecture

This directory contains the modular components of the AppFlowy Cloud diagnostic tool. The original monolithic script has been refactored into separate, maintainable modules.

## Architecture Overview

```
script/
├── diagnose_appflowy.sh          # Main orchestration script
└── lib/                           # Module library
    ├── utils.sh                   # Utility functions
    ├── check_containers.sh        # Container checks
    ├── check_config.sh            # Configuration validation
    ├── check_health.sh            # Health endpoint checks
    ├── check_functional.sh        # Functional tests
    ├── check_logs.sh              # Log analysis
    └── report.sh                  # Report generation
```

## Module Descriptions

### utils.sh (5.0K)
**Purpose:** Core utility functions used across all modules

**Functions:**
- `print_header()` - Formatted header output
- `print_success()` - Success message with green checkmark
- `print_warning()` - Warning message with yellow icon
- `print_error()` - Error message with red X
- `print_info()` - Informational message with blue icon
- `print_verbose()` - Verbose output (only shown with -v flag)
- `mask_sensitive()` - Mask sensitive data in output
- `show_help()` - Display help message
- `parse_arguments()` - Parse command-line arguments
- `load_env_vars()` - Load environment variables from .env
- `extract_url_scheme()` - Parse URL scheme (http/https)
- `extract_url_host()` - Parse URL host
- `extract_url_path()` - Parse URL path

### check_containers.sh (7.7K)
**Purpose:** Docker and container-related checks

**Functions:**
- `check_docker()` - Verify Docker installation and daemon status
- `check_docker_compose()` - Verify Docker Compose availability
- `detect_compose_file()` - Auto-detect docker-compose.yml file
- `check_env_file()` - Verify .env file exists
- `get_compose_command()` - Get cached Docker Compose command
- `check_container_status()` - Check status of all containers
- `check_service_versions()` - Extract service version information

### check_config.sh (42K)
**Purpose:** Configuration validation and verification

**Functions:**
- `check_duplicate_env_keys()` - Find duplicate environment keys
- `check_legacy_env_vars()` - Detect deprecated AF_* variables
- `check_jwt_secrets()` - Validate JWT secret configuration
- `check_database_urls()` - Verify database connection strings
- `check_base_urls()` - Validate base URL configuration
- `check_scheme_consistency()` - Check HTTP/HTTPS consistency
- `check_gotrue_configuration()` - Validate GoTrue settings
- `check_user_auth_flow()` - Check authentication flow settings
- `check_admin_credentials()` - Verify admin credentials
- `check_smtp_configuration()` - Validate SMTP settings
- `check_ai_server_configuration()` - Verify AI server settings
- `check_nginx_websocket_config()` - Check WebSocket proxy config
- `check_ssl_certificate()` - Validate SSL certificates
- `check_production_https_websocket()` - Check HTTPS/WSS for production
- `check_websocket_cors_headers()` - Verify CORS headers
- `check_plan_limits()` - Check plan and resource limits
- `check_url_scheme_alignment()` - Verify URL scheme consistency
- `check_websocket_url()` - Validate WebSocket URLs

### check_health.sh (3.8K)
**Purpose:** Health endpoint monitoring

**Functions:**
- `check_health_endpoint()` - Check a single health endpoint
- `check_health_endpoints()` - Check all service health endpoints

### check_functional.sh (18K)
**Purpose:** Functional tests and API validation

**Functions:**
- `check_minio_storage()` - Test S3/Minio storage functionality
- `check_database_tables()` - Verify database schema
- `check_api_endpoints()` - Test API endpoint accessibility
- `check_websocket_endpoint()` - Test WebSocket connectivity
- `check_admin_frontend()` - Verify admin frontend accessibility
- `check_collaboration_data()` - Test collaboration features
- `check_ai_service()` - Test AI service functionality
- `check_published_features()` - Check published/public features
- `check_websocket_connection_simulation()` - Simulate WebSocket connection
- `run_functional_tests()` - Orchestrate all functional tests

### check_logs.sh (19K)
**Purpose:** Log analysis and error detection

**Functions:**
- `check_admin_frontend_connectivity()` - Analyze admin frontend logs
- `check_admin_frontend_errors()` - Find admin frontend errors
- `check_gotrue_auth_errors()` - Find GoTrue authentication errors
- `check_container_errors()` - Scan all containers for errors
- `extract_container_crash_summary()` - Summarize container crashes
- `analyze_service_logs()` - Deep analysis of service logs

### report.sh (9.1K)
**Purpose:** Report generation and recommendations

**Functions:**
- `generate_report()` - Create diagnostic report file
- `generate_recommendations()` - Generate actionable recommendations

## Usage

### Running the Main Script
```bash
# Basic diagnostic
./script/diagnose_appflowy.sh

# Verbose with logs
./script/diagnose_appflowy.sh -v -l

# Quick check (skip slow tests)
./script/diagnose_appflowy.sh --quick

# Custom compose file
./script/diagnose_appflowy.sh -f docker-compose-dev.yml
```

### Using Individual Modules
Modules can be sourced independently for custom diagnostic scripts:

```bash
#!/bin/bash

# Source only what you need
source script/lib/utils.sh
source script/lib/check_containers.sh

# Use the functions
check_docker
check_container_status
```

## Adding New Checks

### 1. Add Function to Appropriate Module
```bash
# In script/lib/check_config.sh
check_my_new_feature() {
    print_verbose "Checking my new feature..."

    # Your check logic here
    if [[ condition ]]; then
        print_success "Feature OK"
        return 0
    else
        print_error "Feature failed"
        return 1
    fi
}
```

### 2. Call Function in Main Script
```bash
# In script/diagnose_appflowy.sh main() function
check_my_new_feature
```

## Backup

The original monolithic script is preserved as:
```
script/diagnose_appflowy.sh.backup
```

## Benefits of Modular Architecture

1. **Maintainability**: Each module has a clear, focused purpose
2. **Testability**: Modules can be tested independently
3. **Reusability**: Functions can be used in other scripts
4. **Readability**: Easier to navigate and understand
5. **Extensibility**: New checks can be added without affecting existing code
6. **Collaboration**: Multiple developers can work on different modules

## Version

Current version: 2.0.0 (Modular architecture)
Previous version: 1.0.0 (Monolithic)

## Contributing

When contributing new checks:
1. Place them in the appropriate module
2. Follow the existing code style
3. Use the print_* functions for output
4. Document the function in this README
5. Test the module independently before integration
