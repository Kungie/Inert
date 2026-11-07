#!/bin/sh
# Builds the browser and its Servo helper process into the same target dir.
# inert-servo-host is a separate Cargo workspace so Servo never shares a
# dependency graph with the browser. Usage: scripts/build.sh [--release]
set -e
cd "$(dirname "$0")/.."
cargo build -q $1 -p inert-browser
cargo build -q $1 --manifest-path crates/inert-servo-host/Cargo.toml --target-dir target
