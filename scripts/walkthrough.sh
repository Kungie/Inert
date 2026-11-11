#!/bin/sh
# Runs the browser in demo mode through its feature walkthrough and captures
# each screen. Demo mode uses a throwaway profile, fake VPN data and no API
# keys, so the images never contain personal data.
# Usage: scripts/walkthrough.sh <output-dir>   (demo sites must be running)
set -e
cd "$(dirname "$0")/.."
OUT="${1:-target/walkthrough}"
export INERT_SHOT_RECT="${INERT_SHOT_RECT:-40,40,1440,900}"
mkdir -p "$OUT"
rm -f "$OUT"/*.ready "$OUT"/DONE
BIN="${BIN:-./target/debug/inert-browser}"
[ "$BIN" = "./target/debug/inert-browser" ] && scripts/build.sh
"$BIN" --demo --shot "$OUT" &
PID=$!
echo "t_s,browser_rss_mb,webkit_rss_mb,last_screen" > "$OUT/memory.csv"
T0=$(date +%s)
LAST=start
while kill -0 $PID 2>/dev/null; do
  for f in "$OUT"/*.ready; do
    [ -e "$f" ] || continue
    name=$(basename "$f" .ready)
    sleep 0.3
    screencapture -x -R "$INERT_SHOT_RECT" "$OUT/$name.png"
    rm -f "$f"
    LAST=$name
    echo "captured $name"
  done
  [ -e "$OUT/DONE" ] && break
  B=$(ps -o rss= -p $PID 2>/dev/null | awk '{print int($1/1024)}')
  W=$(ps -axo rss=,comm= | grep -i "WebKit.WebContent\|WebKit.Networking\|WebKit.GPU" | awk '{s+=$1} END {print int(s/1024)}')
  echo "$(( $(date +%s) - T0 )),$B,$W,$LAST" >> "$OUT/memory.csv"
  sleep 0.2
done
wait $PID 2>/dev/null || true
rm -f "$OUT/DONE"
