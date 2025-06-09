#!/bin/bash

# =============================================================================
# AppFlowy Cloud - Environment File Generator
# =============================================================================
# This script generates a .env file from deploy.env or dev.env with optional
# environment-specific secret files for private values.
#
# Usage: ./script/generate_env.sh
#
# Features:
# - Interactive selection of base environment file (deploy.env or dev.env)
# - Optional environment-specific secret files (.env.deploy.secret or .env.dev.secret)
# - Generates final .env file in the project root
# =============================================================================

set -e  # Exit on any error

# Color codes for better output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Project root directory (one level up from script directory)
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Function to print colored output
print_header() {
    echo -e "${BLUE}=================================${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}=================================${NC}"
}

print_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

print_warning() {
    echo -e "${YELLOW}⚠ $1${NC}"
}

print_error() {
    echo -e "${RED}✗ $1${NC}"
}

print_info() {
    echo -e "${CYAN}ℹ $1${NC}"
}

# Function to check if file exists
check_file_exists() {
    local file_path="$1"
    if [[ ! -f "$file_path" ]]; then
        print_error "File does not exist: $file_path"
        return 1
    fi
    return 0
}

# Function to display menu and get user selection
select_env_file() {
    print_header "Select Base Environment File"
    echo "Please choose which environment configuration to use as base:"
    echo
    echo "1) deploy.env  - Production deployment configuration"
    echo "2) dev.env     - Development environment configuration"
    echo "3) Exit"
    echo
    
    while true; do
        read -p "Enter your choice (1-3): " choice
        case $choice in
            1)
                selected_env="deploy.env"
                break
                ;;
            2)
                selected_env="dev.env"
                break
                ;;
            3)
                print_info "Exiting..."
                exit 0
                ;;
            *)
                print_error "Invalid choice. Please enter 1, 2, or 3."
                ;;
        esac
    done
    
    # Verify the selected file exists
    local env_file_path="$PROJECT_ROOT/$selected_env"
    if ! check_file_exists "$env_file_path"; then
        print_error "Selected environment file '$selected_env' not found in project root."
        exit 1
    fi
    
    print_success "Selected: $selected_env"
    echo
}

# Function to check for environment-specific secret file
check_env_secret_file() {
    local env_type=""
    if [[ "$selected_env" == "deploy.env" ]]; then
        env_type="deploy"
    elif [[ "$selected_env" == "dev.env" ]]; then
        env_type="dev"
    fi
    
    local secret_file_path="$PROJECT_ROOT/.env.${env_type}.secret"
    if [[ -f "$secret_file_path" ]]; then
        echo "$secret_file_path"
    fi
}

# Function to select private secret file
select_private_secret_file() {
    print_header "Use Private Secret File (Optional)"
    
    echo "Private secret files allow you to override specific environment variables"
    echo "with sensitive values (like API keys, passwords, etc.)"
    echo
    
    # Check if environment-specific secret file exists
    local secret_file_path=$(check_env_secret_file)
    local env_type=""
    if [[ "$selected_env" == "deploy.env" ]]; then
        env_type="deploy"
    elif [[ "$selected_env" == "dev.env" ]]; then
        env_type="dev"
    fi
    
    if [[ -n "$secret_file_path" ]]; then
        print_success "Found .env.${env_type}.secret file in project root"
        echo
        echo "1) Use .env.${env_type}.secret for private values"
        echo "2) Continue without private secrets"
        echo "3) Exit"
        echo
        
        while true; do
            read -p "Enter your choice (1-3): " choice
            case $choice in
                1)
                    selected_secret_file="$secret_file_path"
                    break
                    ;;
                2)
                    selected_secret_file=""
                    break
                    ;;
                3)
                    print_info "Exiting..."
                    exit 0
                    ;;
                *)
                    print_error "Invalid choice. Please enter 1, 2, or 3."
                    ;;
            esac
        done
    else
        print_warning ".env.${env_type}.secret file not found in project root."
        echo "You can create a .env.${env_type}.secret file with KEY=VALUE pairs to override"
        echo "sensitive environment variables and run this script again."
        echo
        echo "1) Continue without private secrets"
        echo "2) Exit"
        echo
        
        while true; do
            read -p "Enter your choice (1-2): " choice
            case $choice in
                1)
                    selected_secret_file=""
                    break
                    ;;
                2)
                    print_info "Exiting..."
                    exit 0
                    ;;
                *)
                    print_error "Invalid choice. Please enter 1 or 2."
                    ;;
            esac
        done
    fi
    
    if [[ -n "$selected_secret_file" ]]; then
        print_success "Will use .env.${env_type}.secret for private values"
    else
        print_info "Continuing without private secrets."
    fi
    echo
}

# Function to extract keys from a file
extract_keys_from_file() {
    local file_path="$1"
    local keys=()
    
    # Extract environment variable keys (handle various formats)
    while IFS= read -r line; do
        # Skip comments and empty lines
        if [[ "$line" =~ ^[[:space:]]*# ]] || [[ "$line" =~ ^[[:space:]]*$ ]]; then
            continue
        fi
        
        # Extract key from KEY=VALUE format
        if [[ "$line" =~ ^[[:space:]]*([A-Z_][A-Z0-9_]*)[[:space:]]*= ]]; then
            local key="${BASH_REMATCH[1]}"
            keys+=("$key")
        fi
    done < "$file_path"
    
    echo "${keys[@]}"
}

# Function to generate the final .env file
generate_env_file() {
    print_header "Generating .env File"
    
    local base_env_path="$PROJECT_ROOT/$selected_env"
    local output_path="$PROJECT_ROOT/.env"
    
    # Check if .env file already exists
    if [[ -f "$output_path" ]]; then
        print_warning ".env file already exists in project root"
        echo
        echo "1) Override existing .env file"
        echo "2) Cancel and exit"
        echo
        
        while true; do
            read -p "Enter your choice (1-2): " choice
            case $choice in
                1)
                    print_info "Will override existing .env file..."
                    # Create backup of existing .env file
                    local backup_path="$PROJECT_ROOT/.env.backup.$(date +%Y%m%d_%H%M%S)"
                    cp "$output_path" "$backup_path"
                    print_success "Created backup: $(basename "$backup_path")"
                    break
                    ;;
                2)
                    print_info "Operation cancelled by user."
                    print_info "Your existing .env file has been preserved."
                    exit 0
                    ;;
                *)
                    print_error "Invalid choice. Please enter 1 or 2."
                    ;;
            esac
        done
        echo
    fi
    
    # Start with base environment file
    print_info "Copying base environment from $selected_env..."
    cp "$base_env_path" "$output_path"
    
    if [[ -n "$selected_secret_file" ]]; then
        print_info "Processing private secret file..."
        
        # Extract keys from secret file
        local secret_keys=()
        while IFS= read -r key; do
            if [[ -n "$key" ]]; then
                secret_keys+=("$key")
            fi
        done < <(extract_keys_from_file "$selected_secret_file" | tr ' ' '\n')
        
        if [[ ${#secret_keys[@]} -eq 0 ]]; then
            print_warning "No valid environment variables found in private secret file."
        else
            print_info "Found ${#secret_keys[@]} keys in private secret file: ${secret_keys[*]}"
            
            # Create a temporary file for processing
            local temp_file=$(mktemp)
            cp "$output_path" "$temp_file"
            
            # Replace keys that exist in both files
            local replaced_count=0
            while IFS= read -r line; do
                # Skip comments and empty lines in secret file
                if [[ "$line" =~ ^[[:space:]]*# ]] || [[ "$line" =~ ^[[:space:]]*$ ]]; then
                    continue
                fi
                
                # Extract key-value pair from secret file
                if [[ "$line" =~ ^[[:space:]]*([A-Z_][A-Z0-9_]*)[[:space:]]*=[[:space:]]*(.*) ]]; then
                    local secret_key="${BASH_REMATCH[1]}"
                    local secret_value="${BASH_REMATCH[2]}"
                    
                    # Skip if secret value is empty
                    if [[ -z "$secret_value" ]]; then
                        print_warning "Key '$secret_key' has empty value in secret file - skipping"
                        continue
                    fi
                    
                    # Check if this key exists in the base environment file
                    if grep -q "^[[:space:]]*${secret_key}[[:space:]]*=" "$base_env_path"; then
                        # Replace the key in the output file using a line-by-line approach
                        local temp_file_new="${temp_file}.new"
                        local found_and_replaced=false
                        
                        while IFS= read -r env_line; do
                            # Check if this line matches our target key
                            if [[ "$env_line" =~ ^[[:space:]]*${secret_key}[[:space:]]*= ]]; then
                                # Replace the entire line with the new key=value
                                echo "${secret_key}=${secret_value}" >> "$temp_file_new"
                                found_and_replaced=true
                            else
                                # Keep the original line
                                echo "$env_line" >> "$temp_file_new"
                            fi
                        done < "$temp_file"
                        
                        if [[ "$found_and_replaced" == true ]]; then
                            mv "$temp_file_new" "$temp_file"
                            ((replaced_count++))
                            print_success "Replaced: $secret_key"
                        else
                            rm -f "$temp_file_new"
                            print_warning "Failed to find and replace: $secret_key"
                        fi
                    else
                        print_warning "Key '$secret_key' from secret file not found in base environment file - skipping"
                    fi
                fi
            done < "$selected_secret_file"
            
            # Replace the original output file
            mv "$temp_file" "$output_path"
            
            if [[ $replaced_count -gt 0 ]]; then
                print_success "Successfully replaced $replaced_count environment variables with private values"
            else
                print_warning "No matching keys found between base environment and private secret files"
            fi
        fi
    fi
    
    print_success "Generated .env file successfully!"
    print_info "Output file: $output_path"
    
    # Show summary
    local total_vars=$(grep -c '^[A-Z_][A-Z0-9_]*=' "$output_path" 2>/dev/null || echo "0")
    print_info "Total environment variables: $total_vars"
    
    echo
    print_header "Summary"
    echo "Base environment file: $selected_env"
    if [[ -n "$selected_secret_file" ]]; then
        local env_type=""
        if [[ "$selected_env" == "deploy.env" ]]; then
            env_type="deploy"
        elif [[ "$selected_env" == "dev.env" ]]; then
            env_type="dev"
        fi
        echo "Private secret file: .env.${env_type}.secret"
    else
        echo "Private secret file: None"
    fi
    echo "Output file: .env"
    # Check if backup was created (look for recent backup files)
    local recent_backup=$(find "$PROJECT_ROOT" -name ".env.backup.*" -newer "$PROJECT_ROOT/$selected_env" 2>/dev/null | head -1)
    if [[ -n "$recent_backup" ]]; then
        echo "Backup created: $(basename "$recent_backup")"
    fi
    echo "Total variables: $total_vars"
    echo
    print_success "Environment file generation complete!"
}

# Function to show help
show_help() {
    echo "AppFlowy Cloud Environment File Generator"
    echo
    echo "This script helps you generate a .env file from either deploy.env or dev.env"
    echo "with optional environment-specific secret file integration."
    echo
    echo "Usage: $0 [OPTIONS]"
    echo
    echo "Options:"
    echo "  -h, --help    Show this help message"
    echo
    echo "Interactive Features:"
    echo "  • Select base environment file (deploy.env or dev.env)"
    echo "  • Optionally use environment-specific secret files for private values"
    echo "  • Automatically replace matching keys with private values"
    echo "  • Generate final .env file in project root"
    echo
    echo "Private Secret File Format:"
    echo "  Create .env.deploy.secret or .env.dev.secret files with KEY=VALUE format."
    echo "  Only keys that exist in both the base environment file and secret file"
    echo "  will be replaced."
    echo
    echo "Examples:"
    echo "  $0                    # Run interactive mode"
    echo "  $0 --help           # Show this help"
}

# Main execution
main() {
    # Parse command line arguments
    case "${1:-}" in
        -h|--help)
            show_help
            exit 0
            ;;
        "")
            # Interactive mode - continue with script
            ;;
        *)
            print_error "Unknown option: $1"
            show_help
            exit 1
            ;;
    esac
    
    print_header "AppFlowy Cloud Environment Generator"
    echo "This script will help you generate a .env file from your environment templates."
    echo
    
    # Step 1: Select base environment file
    select_env_file
    
    # Step 2: Select private secret file (optional)
    select_private_secret_file
    
    # Step 3: Generate the final .env file
    generate_env_file
    
    echo
    print_info "You can now use the generated .env file with your AppFlowy Cloud deployment."
    print_info "Remember to never commit .env files containing sensitive information to version control!"
}

# Run main function
main "$@"
