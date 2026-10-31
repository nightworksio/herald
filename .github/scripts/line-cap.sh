#!/usr/bin/env bash
# Every tracked file within the line cap. Exit 0 when all are, 1 naming each
# that is not.
set -euo pipefail

cap=1000
over=0
while IFS= read -r -d '' file; do
  case "$file" in
    *.lock | */package-lock.json | package-lock.json) continue ;;
    *.png | *.jpg | *.jpeg | *.gif | *.ico | *.svg | *.webp) continue ;;
  esac
  [ -f "$file" ] || continue
  lines=$(wc -l < "$file")
  if [ "$lines" -gt "$cap" ]; then
    echo "::error file=${file}::${file} has ${lines} lines; the cap is ${cap}"
    over=1
  fi
done < <(git ls-files -z)
exit "$over"
