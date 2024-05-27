#!/bin/bash

# Show all Redis stream values matching the given prefix
# Usage: ./show_redis_stream_values.sh stream_prefix

# Check if the correct number of arguments is provided
if [ "$#" -ne 1 ]; then
    echo "Usage: $0 <stream_prefix>"
    exit 1
fi

# Check if redis-cli is installed
if ! command -v redis-cli &> /dev/null
then
    echo "redis-cli could not be found. Please install Redis CLI."
    exit 1
fi

# Input stream prefix
stream_prefix=$1

# Initialize cursor for SCAN
cursor=0

while :; do
    # Scan for keys with the specified prefix
    result=$(redis-cli scan $cursor match "${stream_prefix}*")

    # Extract cursor and keys
    cursor=$(echo "$result" | head -n 1)
    keys=$(echo "$result" | tail -n +2)
    echo "Found keys: $(echo $keys | wc -w)"

    # Loop through the keys and show entries
    for key in $keys; do
        echo "Entries for stream key: $key"
        redis-cli xrange $key - + | while read -r entry_id fields; do
            echo "Entry ID: $entry_id"
            echo "Fields: $fields"
        done
    done

    # Break loop if cursor is 0
    [ "$cursor" -eq 0 ] && break
done
