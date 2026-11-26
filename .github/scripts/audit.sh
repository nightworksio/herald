#!/usr/bin/env bash
# cargo audit over each lockfile given, ignoring the advisories deny.toml
# ignores, so one list holds them for both tools. osv-scanner.toml, which
# OpenSSF Scorecard reads, must name the same advisories.
set -euo pipefail

mapfile -t ignored < <(grep -o 'RUSTSEC-[0-9]*-[0-9]*' deny.toml)
mirrored=$(grep -o 'RUSTSEC-[0-9]*-[0-9]*' osv-scanner.toml | sort || true)
if [[ "$mirrored" != "$(printf '%s\n' "${ignored[@]}" | sort)" ]]; then
  echo "osv-scanner.toml must ignore exactly the advisories deny.toml ignores" >&2
  exit 1
fi
args=(--deny warnings)
for id in "${ignored[@]}"; do
  args+=(--ignore "$id")
done
for lockfile in "$@"; do
  cargo audit "${args[@]}" --file "$lockfile"
done
