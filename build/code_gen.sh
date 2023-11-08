#!/usr/bin/env bash
set -x
set -eo pipefail

# Generate protobuf files for realtime-entity crate.
cargo build -p realtime-entity