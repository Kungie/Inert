#!/bin/sh
# Builds the browser and its Servo helper process into the same target dir.
# inert-servo-host is a separate Cargo workspace so Servo never shares a
# dependency graph with the browser. Usage: scripts/build.sh [--release]
set -e
cd "$(dirname "$0")/.."
cargo build -q $1 -p inert-browser
cargo build -q $1 --manifest-path crates/inert-servo-host/Cargo.toml --target-dir target
# The push-to-talk helper (Swift, Apple's Speech framework). Its Info.plist
# is embedded so macOS can show why it wants the microphone.
if command -v swiftc >/dev/null 2>&1; then
  OUT=target/$([ "$1" = "--release" ] && echo release || echo debug)
  mkdir -p "$OUT"
  swiftc -O crates/inert-dictate/main.swift -o "$OUT/inert-dictate" \
    -Xlinker -sectcreate -Xlinker __TEXT -Xlinker __info_plist -Xlinker crates/inert-dictate/Info.plist
fi
