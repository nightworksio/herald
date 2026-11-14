#!/usr/bin/env bash
# cargo audit over each lockfile given, ignoring the advisories deny.toml
# ignores, so one list holds them for both tools.
set -euo pipefail

mapfile -t ignored < <(grep -o 'RUSTSEC-[0-9]*-[0-9]*' deny.toml)
args=(--deny warnings)
for id in "${ignored[@]}"; do
  args+=(--ignore "$id")
done
for lockfile in "$@"; do
  cargo audit "${args[@]}" --file "$lockfile"
done
