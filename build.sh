#!/bin/sh

set -ex

cargo +nightly build --target wasm32-unknown-unknown --release
wasm-bindgen target/wasm32-unknown-unknown/debug/wasm_example.wasm --out-dir .
# npm install
# npm run serve
