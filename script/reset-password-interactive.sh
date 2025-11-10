#!/bin/bash
# Interactive Password Reset Script for GoTrue
# Usage: ./reset-password-interactive.sh

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

echo -e "${CYAN}=== GoTrue Interactive Password Reset ===${NC}"
echo ""

# Check for required tools
echo "Checking dependencies..."

# Check Python3
if ! command -v python3 &> /dev/null; then
    echo -e "${RED}ERROR: python3 is not installed${NC}"
    echo "Please install Python 3 first:"
    echo "  - macOS: brew install python3"
    echo "  - Ubuntu/Debian: sudo apt-get install python3"
    echo "  - CentOS/RHEL: sudo yum install python3"
    exit 1
fi

# Check pip3
if ! command -v pip3 &> /dev/null; then
    echo -e "${RED}ERROR: pip3 is not installed${NC}"
    exit 1
fi

# Check Docker
if ! command -v docker &> /dev/null; then
    echo -e "${RED}ERROR: docker is not installed${NC}"
    echo "Please install Docker first: https://docs.docker.com/get-docker/"
    exit 1
fi

# Check if bcrypt is available
if ! python3 -c "import bcrypt" 2>/dev/null; then
    echo ""
    echo -e "${YELLOW}WARNING: Python 'bcrypt' library is not installed${NC}"
    echo "This script requires the 'bcrypt' library to generate password hashes."
    echo ""
    read -p "Would you like to install it now? (y/n) " -n 1 -r
    echo ""

    if [[ $REPLY =~ ^[Yy]$ ]]; then
        echo "Installing bcrypt library..."
        if pip3 install bcrypt --quiet --user; then
            echo -e "${GREEN}✓${NC} bcrypt installed successfully"
        else
            echo -e "${RED}ERROR: Failed to install bcrypt${NC}"
            exit 1
        fi
    else
        echo -e "${RED}ERROR: Cannot proceed without bcrypt library${NC}"
        exit 1
    fi
fi

echo ""
echo -e "${CYAN}Finding PostgreSQL containers...${NC}"
echo ""

# Find all containers with postgres in the image name or container name
POSTGRES_CONTAINERS=$(docker ps -a --format '{{.Names}}|{{.Image}}|{{.Status}}' | grep -i postgres)

if [ -z "$POSTGRES_CONTAINERS" ]; then
    echo -e "${RED}ERROR: No PostgreSQL containers found${NC}"
    echo ""
    echo "Available containers:"
    docker ps -a --format "table {{.Names}}\t{{.Image}}\t{{.Status}}"
    exit 1
fi

# Count containers
CONTAINER_COUNT=$(echo "$POSTGRES_CONTAINERS" | wc -l | tr -d ' ')

if [ "$CONTAINER_COUNT" -eq 1 ]; then
    # Only one container, use it automatically
    SELECTED_CONTAINER=$(echo "$POSTGRES_CONTAINERS" | cut -d'|' -f1)
    CONTAINER_IMAGE=$(echo "$POSTGRES_CONTAINERS" | cut -d'|' -f2)
    CONTAINER_STATUS=$(echo "$POSTGRES_CONTAINERS" | cut -d'|' -f3)

    echo -e "${GREEN}Found 1 PostgreSQL container:${NC}"
    echo "  Name: $SELECTED_CONTAINER"
    echo "  Image: $CONTAINER_IMAGE"
    echo "  Status: $CONTAINER_STATUS"
    echo ""

    # Check if running
    if ! echo "$CONTAINER_STATUS" | grep -q "Up"; then
        echo -e "${YELLOW}WARNING: Container is not running${NC}"
        read -p "Do you want to continue anyway? (y/n) " -n 1 -r
        echo ""
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    fi
else
    # Multiple containers - let user choose
    echo -e "${BLUE}Found $CONTAINER_COUNT PostgreSQL containers:${NC}"
    echo ""
    printf "${CYAN}%-4s %-40s %-30s %-20s${NC}\n" "No." "Container Name" "Image" "Status"
    echo "--------------------------------------------------------------------------------------------------------"

    declare -a container_names
    index=1
    while IFS='|' read -r name image status; do
        container_names[$index]="$name"

        # Truncate long names/images for display
        display_name="${name:0:40}"
        display_image="${image:0:30}"
        display_status="${status:0:20}"

        printf "%-4s %-40s %-30s %-20s\n" "$index" "$display_name" "$display_image" "$display_status"
        ((index++))
    done <<< "$POSTGRES_CONTAINERS"

    echo ""

    # Prompt user to select
    while true; do
        read -p "Enter the number of the PostgreSQL container to use (or 'q' to quit): " selection

        if [ "$selection" = "q" ] || [ "$selection" = "Q" ]; then
            echo "Cancelled."
            exit 0
        fi

        # Check if selection is a valid number
        if ! [[ "$selection" =~ ^[0-9]+$ ]]; then
            echo -e "${RED}Invalid input. Please enter a number.${NC}"
            continue
        fi

        if [ "$selection" -lt 1 ] || [ "$selection" -gt "$CONTAINER_COUNT" ]; then
            echo -e "${RED}Invalid selection. Please choose between 1 and $CONTAINER_COUNT.${NC}"
            continue
        fi

        break
    done

    SELECTED_CONTAINER="${container_names[$selection]}"

    # Get container details
    CONTAINER_INFO=$(docker ps -a --format '{{.Names}}|{{.Image}}|{{.Status}}' --filter "name=^${SELECTED_CONTAINER}$")
    CONTAINER_IMAGE=$(echo "$CONTAINER_INFO" | cut -d'|' -f2)
    CONTAINER_STATUS=$(echo "$CONTAINER_INFO" | cut -d'|' -f3)

    echo ""
    echo -e "${GREEN}Selected container:${NC}"
    echo "  Name: $SELECTED_CONTAINER"
    echo "  Image: $CONTAINER_IMAGE"
    echo "  Status: $CONTAINER_STATUS"
    echo ""

    # Check if running
    if ! echo "$CONTAINER_STATUS" | grep -q "Up"; then
        echo -e "${YELLOW}WARNING: Container is not running${NC}"
        read -p "Do you want to continue anyway? (y/n) " -n 1 -r
        echo ""
        if [[ ! $REPLY =~ ^[Yy]$ ]]; then
            exit 1
        fi
    fi
fi

# Get database credentials
PGUSER=$(docker exec "$SELECTED_CONTAINER" printenv POSTGRES_USER 2>/dev/null || echo "postgres")
PGDATABASE=$(docker exec "$SELECTED_CONTAINER" printenv POSTGRES_DB 2>/dev/null || echo "postgres")

echo -e "${GREEN}✓${NC} Using PostgreSQL container: $SELECTED_CONTAINER"
echo -e "${GREEN}✓${NC} Database: $PGDATABASE (User: $PGUSER)"
echo ""

# Fetch users from database
echo -e "${CYAN}Fetching users from database...${NC}"
echo ""

# Get users data into a temporary file
TEMP_FILE=$(mktemp)
docker exec "$SELECTED_CONTAINER" psql -U "$PGUSER" -d "$PGDATABASE" -t -A -F'|' -c "
SELECT
    email,
    to_char(created_at, 'YYYY-MM-DD HH24:MI:SS'),
    to_char(last_sign_in_at, 'YYYY-MM-DD HH24:MI:SS'),
    CASE WHEN confirmed_at IS NOT NULL THEN 'Yes' ELSE 'No' END,
    CASE
        WHEN is_super_admin = true THEN 'Yes'
        WHEN role LIKE '%admin%' THEN 'Yes'
        ELSE 'No'
    END
FROM auth.users
ORDER BY created_at DESC;
" > "$TEMP_FILE"

# Check if there are any users
if [ ! -s "$TEMP_FILE" ]; then
    echo -e "${RED}No users found in the database${NC}"
    rm "$TEMP_FILE"
    exit 1
fi

# Display users in a table format
echo -e "${BLUE}Available Users:${NC}"
echo ""
printf "${CYAN}%-4s %-30s %-20s %-20s %-10s %-8s${NC}\n" "No." "Email" "Created" "Last Sign-in" "Confirmed" "Admin"
echo "------------------------------------------------------------------------------------------------------------"

# Read users into an array and display them
declare -a emails
index=1
while IFS='|' read -r email created last_signin confirmed admin; do
    # Handle null/empty last_signin
    if [ -z "$last_signin" ] || [ "$last_signin" = " " ]; then
        last_signin="Never"
    fi

    emails[$index]="$email"
    printf "%-4s %-30s %-20s %-20s %-10s %-8s\n" "$index" "$email" "$created" "$last_signin" "$confirmed" "$admin"
    ((index++))
done < "$TEMP_FILE"

total_users=$((index - 1))
echo ""
echo -e "${GREEN}Total users: $total_users${NC}"
echo ""

# Prompt user to select
while true; do
    read -p "Enter the number of the user to reset password (or 'q' to quit): " selection

    if [ "$selection" = "q" ] || [ "$selection" = "Q" ]; then
        echo "Cancelled."
        rm "$TEMP_FILE"
        exit 0
    fi

    # Check if selection is a valid number
    if ! [[ "$selection" =~ ^[0-9]+$ ]]; then
        echo -e "${RED}Invalid input. Please enter a number.${NC}"
        continue
    fi

    if [ "$selection" -lt 1 ] || [ "$selection" -gt "$total_users" ]; then
        echo -e "${RED}Invalid selection. Please choose between 1 and $total_users.${NC}"
        continue
    fi

    break
done

SELECTED_EMAIL="${emails[$selection]}"
echo ""
echo -e "${GREEN}Selected user:${NC} $SELECTED_EMAIL"
echo ""

# Prompt for new password
while true; do
    read -p "Enter new password: " password1
    echo ""

    if [ ${#password1} -lt 6 ]; then
        echo -e "${RED}Password must be at least 6 characters long${NC}"
        continue
    fi

    read -p "Confirm new password: " password2
    echo ""

    if [ "$password1" != "$password2" ]; then
        echo -e "${RED}Passwords do not match. Please try again.${NC}"
        echo ""
        continue
    fi

    break
done

NEW_PASSWORD="$password1"
echo ""

# Confirm action
echo -e "${YELLOW}You are about to reset the password for:${NC}"
echo -e "  Email: ${GREEN}$SELECTED_EMAIL${NC}"
echo ""
read -p "Are you sure you want to proceed? (yes/no) " -r
echo ""

if [[ ! $REPLY =~ ^[Yy][Ee][Ss]$ ]]; then
    echo "Cancelled."
    rm "$TEMP_FILE"
    exit 0
fi

echo -e "${CYAN}Resetting password...${NC}"
echo ""

# Generate bcrypt hash
echo "Step 1: Generating bcrypt hash..."
BCRYPT_HASH=$(python3 << EOF
import bcrypt
password = "$NEW_PASSWORD"
salt = bcrypt.gensalt(rounds=10)
hashed = bcrypt.hashpw(password.encode('utf-8'), salt)
print(hashed.decode('utf-8'))
EOF
)

if [ -z "$BCRYPT_HASH" ]; then
    echo -e "${RED}ERROR: Failed to generate bcrypt hash${NC}"
    rm "$TEMP_FILE"
    exit 1
fi

echo -e "${GREEN}✓${NC} Hash generated successfully"
echo ""

# Update password in database
echo "Step 2: Updating password in database..."
RESULT=$(docker exec "$SELECTED_CONTAINER" psql -U "$PGUSER" -d "$PGDATABASE" -t -c "
UPDATE auth.users
SET encrypted_password = '$BCRYPT_HASH',
    updated_at = now()
WHERE email = '$SELECTED_EMAIL'
RETURNING email;
" 2>&1)

if [ $? -ne 0 ]; then
    echo -e "${RED}ERROR: Failed to update password in database${NC}"
    echo "Details: $RESULT"
    rm "$TEMP_FILE"
    exit 1
fi

echo -e "${GREEN}✓${NC} Password updated successfully!"
echo ""

# Clean up
rm "$TEMP_FILE"

echo -e "${GREEN}=== Password Reset Complete ===${NC}"
echo ""
echo -e "Email: ${GREEN}$SELECTED_EMAIL${NC}"
echo -e "Status: ${GREEN}Password has been reset${NC}"
echo ""
echo -e "${YELLOW}Important:${NC}"
echo "1. The user can now log in with the new password"
echo "2. Consider notifying the user about the password change"
echo "3. Recommend the user to change their password after logging in"
echo ""
