#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
target="$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)"
cargo build --locked --release --target wasm32-unknown-unknown -p herald-wasm --manifest-path "$root/Cargo.toml"
wasm-bindgen --target web --out-dir "$root/deploy/pkg" "$target/wasm32-unknown-unknown/release/herald_wasm.wasm"
