#!/bin/bash

# delete all Redis keys matching the prefix af_collab_update that are older than October 5, 2023
# ./delete_redis_keys.sh af_collab_update 20231005

# Function to convert date string (YYYYMMDD) to Unix timestamp
date_to_timestamp() {
    date -j -f "%Y%m%d" "$1" "+%s"
}

# Check if the correct number of arguments is provided
if [ "$#" -ne 2 ]; then
    echo "Usage: $0 <key_prefix> <YYYYMMDD>"
    exit 1
fi

# Check if redis-cli is installed
if ! command -v redis-cli &> /dev/null
then
    echo "redis-cli could not be found. Please install Redis CLI."
    exit 1
fi

# Input key prefix and date in YYYYMMDD format
key_prefix=$1
input_date=$2

# Validate input date format
if ! [[ $input_date =~ ^[0-9]{8}$ ]]; then
    echo "Invalid date format. Please use YYYYMMDD."
    exit 1
fi

# Convert input date to Unix timestamp
input_timestamp=$(date_to_timestamp $input_date)

if [ $? -ne 0 ]; then
    echo "Error converting date to timestamp. Please check the input date."
    exit 1
fi

# Initialize cursor for SCAN
cursor=0

while :; do
    # Scan for keys with the specified prefix
    result=$(redis-cli scan $cursor match "${key_prefix}*")

    # Extract cursor and keys
    cursor=$(echo "$result" | head -n 1)
    keys=$(echo "$result" | tail -n +2)

    echo "Number of keys: $(echo $keys | wc -w)"

    # Loop through the keys and delete those older than the input date
    for key in $keys; do
        # Extract timestamp from the key
        timestamp=$(echo $key | awk -F'_' '{print $3}')

        if [ -n "$timestamp" ]; then
            # Convert the timestamp to seconds since epoch
            key_time=$(date -j -f "%Y%m%d" "$timestamp" "+%s" 2>/dev/null)

            if [ $? -eq 0 ] && [ $key_time -lt $input_timestamp ]; then
                # Delete the key if it's older than the input date
                redis-cli DEL $key
                echo "Deleted key: $key"
            fi
        fi
    done

    # Break loop if cursor is 0
    [ "$cursor" -eq 0 ] && break
done
