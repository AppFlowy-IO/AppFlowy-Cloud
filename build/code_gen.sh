#!/usr/bin/env bash
set -x
set -eo pipefail

# Generate protobuf files for collab-rt-entity crate.
cargo build -p collab-rt-entity