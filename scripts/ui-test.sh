#!/bin/sh
# End-to-end UI tests with real input events (see crates/inert-browser/src/drive.rs),
# with memory sampling for each process while they run.
#
# Demo sites must be running (scripts/demo-sites.sh). Uses a throwaway demo
# profile. Writes <dir>/results.json, <dir>/memory.csv and <dir>/driver.log;
# exits non-zero if a check fails.
#
# Memory is the physical footprint (Activity Monitor's "Memory") plus RSS,
# sampled continuously for the browser and the Servo process.
#
# SHOTS=1 also saves documentation screenshots. They are off by default:
# egui-wgpu keeps a capture texture and readback buffers (~36 MB of GPU
# memory) after the first screenshot, which is test overhead, not browser memory.
# Usage: [SHOTS=1] scripts/ui-test.sh <output-dir>
set -e
cd "$(dirname "$0")/.."
OUT="${1:-target/ui-test}"
export INERT_SHOT_RECT="${INERT_SHOT_RECT:-40,40,1440,900}"
mkdir -p "$OUT"
rm -f "$OUT"/*.png "$OUT"/results.json "$OUT"/memory.csv
BIN="${BIN:-./target/release/inert-browser}"
[ -x "$BIN" ] || scripts/build.sh --release
[ "${SHOTS:-0}" = 1 ] || export INERT_DRIVE_NO_SHOTS=1
fp() { footprint -p "$1" 2>/dev/null | awk '/phys_footprint:/{v=$2; u=$3; if(u=="GB")v*=1024; else if(u=="KB")v/=1024; print v; exit}'; }
"$BIN" --demo --drive "$OUT" 2>"$OUT/driver.log" &
PID=$!
echo "t,browser_rss,browser_fp,servo_rss,servo_fp" > "$OUT/memory.csv"
t=0
while kill -0 $PID 2>/dev/null; do
  br=$(( $(ps -o rss= -p $PID 2>/dev/null || echo 0) / 1024 )); bf=$(fp $PID)
  SP=$(pgrep -P $PID -x inert-servo-host | head -1); sr=0; sf=0
  if [ -n "$SP" ]; then sr=$(( $(ps -o rss= -p "$SP" 2>/dev/null || echo 0) / 1024 )); sf=$(fp "$SP"); fi
  [ "$br" -gt 0 ] && echo "$t,$br,${bf:-0},$sr,${sf:-0}" >> "$OUT/memory.csv"
  t=$((t+1))
done
wait $PID 2>/dev/null || true
grep -E "^(PASS|FAIL|UI DRIVER)" "$OUT/driver.log"
python3 - "$OUT/memory.csv" <<'PY'
import csv, sys, statistics as st
rows = list(csv.DictReader(open(sys.argv[1])))
for k, name in [("browser_fp", "browser footprint"), ("servo_fp", "servo footprint"), ("browser_rss", "browser RSS"), ("servo_rss", "servo RSS")]:
    v = [float(r[k]) for r in rows if float(r[k]) > 0]
    if v:
        print(f"MEMORY {name}: median {st.median(v):.0f} MB, max {max(v):.0f} MB ({len(v)} samples)")
PY
python3 -c "import json,sys; r=json.load(open('$OUT/results.json')); sys.exit(0 if r['passed']==r['total'] else 1)"
