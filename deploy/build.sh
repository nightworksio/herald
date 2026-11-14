#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
out="${1:-$root/deploy/pkg}"
features="${2:-}"

for tool in cargo jq wasm-bindgen; do
  if ! command -v "$tool" >/dev/null; then
    echo "deploy/build.sh needs $tool on the PATH" >&2
    exit 1
  fi
done
locked="$(cargo metadata --format-version 1 --manifest-path "$root/Cargo.toml" --locked |
  jq -r 'first(.packages[] | select(.name == "wasm-bindgen") | .version)')"
installed="$(wasm-bindgen --version | awk '{ print $2 }')"
if [ "$installed" != "$locked" ]; then
  echo "deploy/build.sh needs wasm-bindgen $locked, as Cargo.lock holds, and found $installed; install it with: cargo install wasm-bindgen-cli --version $locked --locked" >&2
  exit 1
fi

target="$(cargo metadata --format-version 1 --no-deps --manifest-path "$root/Cargo.toml" | jq -r .target_directory)"
cargo build --locked --release --target wasm32-unknown-unknown -p herald-wasm --features "$features" --manifest-path "$root/Cargo.toml"
wasm-bindgen --target web --out-dir "$out" "$target/wasm32-unknown-unknown/release/herald_wasm.wasm"
