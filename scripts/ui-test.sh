#!/bin/sh
# End-to-end UI tests with real input events (see crates/inert-browser/src/drive.rs).
# Demo sites must be running (scripts/demo-sites.sh). Uses a throwaway demo
# profile. Writes <dir>/results.json and screenshots; exits non-zero on failure.
# Usage: scripts/ui-test.sh <output-dir>
set -e
cd "$(dirname "$0")/.."
OUT="${1:-target/ui-test}"
export INERT_SHOT_RECT="${INERT_SHOT_RECT:-40,40,1440,900}"
mkdir -p "$OUT"
rm -f "$OUT"/*.ready "$OUT"/DONE "$OUT"/results.json
BIN="${BIN:-./target/debug/inert-browser}"
[ "$BIN" = "./target/debug/inert-browser" ] && cargo build -q -p inert-browser
"$BIN" --demo --drive "$OUT" 2>"$OUT/driver.log" &
PID=$!
while kill -0 $PID 2>/dev/null; do
  for f in "$OUT"/*.ready; do
    [ -e "$f" ] || continue
    name=$(basename "$f" .ready)
    sleep 0.3
    screencapture -x -R "$INERT_SHOT_RECT" "$OUT/$name.png"
    rm -f "$f"
  done
  [ -e "$OUT/DONE" ] && break
  sleep 0.2
done
wait $PID 2>/dev/null || true
rm -f "$OUT/DONE"
grep -E "^(PASS|FAIL|UI DRIVER)" "$OUT/driver.log"
python3 -c "import json,sys; r=json.load(open('$OUT/results.json')); sys.exit(0 if r['passed']==r['total'] else 1)"
