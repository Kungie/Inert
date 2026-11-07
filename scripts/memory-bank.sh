#!/bin/sh
# Spec §16 baseline: the browser with only Inert Bank open, window drawing
# (~33 frames/s from the driver), sampled for 30 s after the bank verifies.
# Demo sites must be running. Usage: scripts/memory-bank.sh <output-dir>
set -e
cd "$(dirname "$0")/.."
OUT="${1:-target/memory-bank}"
mkdir -p "$OUT"
BIN="${BIN:-./target/release/inert-browser}"
fp() { footprint -p "$1" 2>/dev/null | awk '/phys_footprint:/{v=$2; u=$3; if(u=="GB")v*=1024; else if(u=="KB")v/=1024; print v; exit}'; }
INERT_DRIVE_SCENARIO=bank-idle INERT_DRIVE_NO_SHOTS=1 INERT_SHOT_RECT="${INERT_SHOT_RECT:-40,40,1440,900}" "$BIN" --demo --drive "$OUT" 2>"$OUT/driver.log" &
PID=$!
echo "t,browser_rss,browser_fp,servo_running" > "$OUT/memory.csv"
t=0
while kill -0 $PID 2>/dev/null; do
  br=$(( $(ps -o rss= -p $PID 2>/dev/null || echo 0) / 1024 )); bf=$(fp $PID)
  sv=$(pgrep -P $PID -x inert-servo-host >/dev/null && echo 1 || echo 0)
  [ "$br" -gt 0 ] && echo "$t,$br,${bf:-0},$sv" >> "$OUT/memory.csv"
  t=$((t+1))
done
grep -E "^(PASS|FAIL|UI DRIVER)" "$OUT/driver.log"
python3 - "$OUT/memory.csv" <<'PY'
import csv, sys, statistics as st
rows = [r for r in csv.DictReader(open(sys.argv[1])) if float(r["browser_fp"]) > 0]
steady = rows[len(rows) // 3:]  # after sign-in and the bank's first screen
for k, name in [("browser_fp", "footprint"), ("browser_rss", "RSS")]:
    v = [float(r[k]) for r in steady]
    print(f"BANK {name}: median {st.median(v):.0f} MB, max {max(v):.0f} MB ({len(v)} samples)")
print("BANK servo process running:", any(r["servo_running"] == "1" for r in rows))
PY
