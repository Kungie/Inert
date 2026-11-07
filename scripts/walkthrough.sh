#!/bin/sh
# Runs the browser in demo mode through its feature walkthrough and captures
# each screen. Demo mode uses a throwaway profile, fake VPN data and no API
# keys, so the images never contain personal data.
# Usage: scripts/walkthrough.sh <output-dir>   (demo sites must be running)
set -e
cd "$(dirname "$0")/.."
OUT="${1:-docs/screenshots}"
export INERT_SHOT_RECT="${INERT_SHOT_RECT:-40,40,1440,900}"
mkdir -p "$OUT"
rm -f "$OUT"/*.ready "$OUT"/DONE
cargo build -q -p inert-browser
./target/debug/inert-browser --demo --shot "$OUT" &
PID=$!
while kill -0 $PID 2>/dev/null; do
  for f in "$OUT"/*.ready; do
    [ -e "$f" ] || continue
    name=$(basename "$f" .ready)
    sleep 0.3
    screencapture -x -R "$INERT_SHOT_RECT" "$OUT/$name.png"
    rm -f "$f"
    echo "captured $name"
  done
  [ -e "$OUT/DONE" ] && break
  sleep 0.2
done
wait $PID 2>/dev/null || true
rm -f "$OUT/DONE"
