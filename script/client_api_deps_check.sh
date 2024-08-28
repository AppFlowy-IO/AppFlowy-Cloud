#!/bin/bash

# Generate the current dependency list
cargo tree > current_deps.txt

<<<<<<< HEAD
BASELINE_COUNT=602
=======
BASELINE_COUNT=609
>>>>>>> main
CURRENT_COUNT=$(cat current_deps.txt | wc -l)

echo "Expected dependency count (baseline): $BASELINE_COUNT"
echo "Current dependency count: $CURRENT_COUNT"

if [ "$CURRENT_COUNT" -gt "$BASELINE_COUNT" ]; then
    echo "Dependency count has increased!"
    exit 1
else
    echo "No increase in dependencies."
fi
