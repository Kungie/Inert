#!/bin/sh
# Starts the three demo inert:// sites. Each one signs its manifest with a
# local dev key, writes a dev trust file for the browser and hot-reloads
# app.inert.json. Ctrl-C stops all three.
set -e
cd "$(dirname "$0")/.."
cargo build -q -p inert-example-bank -p inert-example-shop -p inert-example-notes
trap 'kill 0' INT TERM EXIT
./target/debug/inert-bank 127.0.0.1:7443 &
./target/debug/inert-shop 127.0.0.1:7444 &
./target/debug/inert-notes 127.0.0.1:7445 &
echo "inert://bank.example  inert://shop.example  inert://notes.example"
wait
