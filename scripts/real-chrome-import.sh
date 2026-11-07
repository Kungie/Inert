#!/bin/sh
# Import test against a profile written by the real Google Chrome.
#
# Starts the installed Chrome headless with a throwaway profile and
# --use-mock-keychain (so its Safe Storage key is the known "mock_password"
# and the user's Keychain is never touched), visits two local pages (one sets
# a cookie), adds a bookmark and a saved password through Chrome's own pages,
# then imports that profile: first with the importer alone, then through the
# browser's onboarding screen (demo profile, passwords kept in memory).
# Usage: scripts/real-chrome-import.sh [output-dir]
set -e
cd "$(dirname "$0")/.."
OUT="${1:-target/real-chrome-import}"
CHROME="/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
T=$(mktemp -d)
mkdir -p "$OUT"
python3 scripts/real-chrome/server.py & SRV=$!
"$CHROME" --headless=new --user-data-dir="$T/ud" --use-mock-keychain --no-first-run --no-default-browser-check --remote-debugging-port=9333 about:blank >"$OUT/chrome.log" 2>&1 & CH=$!
node scripts/real-chrome/drive.mjs
sleep 2; kill $CH $SRV 2>/dev/null || true
"$CHROME" --version
INERT_REAL_PROFILE="$T/ud/Default" INERT_REAL_KEY=mock_password cargo test -q -p inert-browser real_chrome_profile -- --ignored --nocapture
INERT_IMPORT_DIR="$T/ud/Default" INERT_IMPORT_KEY=mock_password INERT_DRIVE_SCENARIO=import-real ./target/release/inert-browser --demo --drive "$OUT" 2>&1 | grep -E "^(PASS|FAIL|UI DRIVER)"
rm -rf "$T"
