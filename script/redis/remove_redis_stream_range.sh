#!/bin/bash

# delete all Redis stream entries matching the prefix that are older than the given date range
# Usage: ./remove_redis_stream_range.sh stream_prefix 20231001 20231005 [--verbose]

# Function to convert date string (YYYYMMDD) to Unix timestamp in milliseconds
date_to_timestamp_ms() {
    date -j -f "%Y%m%d" "$1" "+%s000" 2>/dev/null || date -d "$1" "+%s000" 2>/dev/null
}

# Check if the correct number of arguments is provided
if [ "$#" -lt 3 ] || [ "$#" -gt 4 ]; then
    echo "Usage: $0 <stream_prefix> <start_date> <end_date> [--verbose]"
    exit 1
fi

# Check if redis-cli is installed
if ! command -v redis-cli &> /dev/null; then
    echo "redis-cli could not be found. Please install Redis CLI."
    exit 1
fi

# Input stream prefix and date range in YYYYMMDD format
stream_prefix=$1
start_date=$2
end_date=$3
verbose=false

# Check for verbose flag
if [ "$#" -eq 4 ] && [ "$4" == "--verbose" ]; then
    verbose=true
fi

# Validate input date format
if ! [[ $start_date =~ ^[0-9]{8}$ ]] || ! [[ $end_date =~ ^[0-9]{8}$ ]]; then
    echo "Invalid date format. Please use YYYYMMDD."
    exit 1
fi

# Convert input dates to Unix timestamps in milliseconds
start_timestamp=$(date_to_timestamp_ms $start_date)
end_timestamp=$(date_to_timestamp_ms $end_date)

if [ $? -ne 0 ] || [ -z "$start_timestamp" ] || [ -z "$end_timestamp" ]; then
    echo "Error converting date to timestamp. Please check the input dates."
    exit 1
fi

# Construct Redis stream IDs
start_id="${start_timestamp}-0"
end_id="${end_timestamp}-0"

# Initialize cursor for SCAN
cursor=0

while :; do
    # Scan for keys with the specified prefix
    result=$(redis-cli scan $cursor match "${stream_prefix}*")

    # Extract cursor and keys
    cursor=$(echo "$result" | head -n 1)
    keys=$(echo "$result" | tail -n +2 | tr -d '\r')

    # Loop through the keys and delete entries within the range
    for key in $keys; do
        if $verbose; then
            echo "Query entries from stream: $key in the range: $start_id to $end_id"
        fi

        # Fetch entries within the range
        entries=$(redis-cli xrange $key $start_id $end_id)

        # Loop through the entries and delete them
        while IFS= read -r entry; do
            entry_id=$(echo $entry | awk '{print $1}')
            if [[ $entry_id =~ ^[0-9]+-[0-9]+$ ]]; then
                redis-cli xdel $key $entry_id
                if $verbose; then
                    echo "Deleted entry: $entry_id from stream: $key"
                fi
            fi
        done <<< "$entries"
    done

    # Break loop if cursor is 0
    [ "$cursor" -eq 0 ] && break
done
