#!/usr/bin/env bash
set -x
set -eo pipefail

cd "$(dirname "$0")/.."
cargo test
