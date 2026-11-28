#!/usr/bin/env bash
# One mutants shard of a ci run (ADR-0070): shard SHARD of SHARDS of the
# mutants MUTATED names, less EXCLUDED, only those EXAMINED matches where it
# is set, a pull request's narrowed to its diff since BASE.
#
# cargo-mutants runs in a cgroup holding at most MEMORY_CAP and no swap: a
# GitHub-hosted runner of a public repository has 16 GB, and the cap leaves
# the runner, the Postgres service and the system the rest, so a mutant that
# allocates without bound has its own test killed and fails it in place of
# taking the runner. A mutant's build may take BUILD_TIMEOUT_MULTIPLIER times
# the baseline's, and its test cargo-mutants' own five times the baseline's.
# Each mutant is named in the log as it starts and as it ends. Each minute
# the log gives the cgroup's anonymous memory, now and the most seen in a
# look every TICK_SECONDS, its page cache, which the kernel reclaims at the
# cap, and the disk's free space; where the free space falls under
# DISK_FLOOR_KIB the run is stopped, naming the mutants running.
set -euo pipefail

readonly MEMORY_CAP=$((12 * 1024 * 1024 * 1024))
readonly DISK_FLOOR_KIB=$((1024 * 1024))
readonly BUILD_TIMEOUT_MULTIPLIER=5
readonly GROUP=/sys/fs/cgroup/herald-mutants
readonly OUT=mutants.out
readonly MOST_ANON="${RUNNER_TEMP}/mutants-most-anon"
readonly TICK_SECONDS=5
readonly TICKS_A_REPORT=12

read -r -a mutated <<< "${MUTATED:?}"
read -r -a excluded <<< "${EXCLUDED:?}"
examined=()
if [[ -n "${EXAMINED:-}" ]]; then
  examined=(--re "$EXAMINED")
fi
scope=()
if [[ "$EVENT" = "pull_request" ]]; then
  git diff "$(git merge-base "$BASE" HEAD)" HEAD > "${RUNNER_TEMP}/change.diff"
  scope=(--in-diff "${RUNNER_TEMP}/change.diff")
fi

if ! grep -qw memory /sys/fs/cgroup/cgroup.subtree_control; then
  echo "::error::The runner's cgroup tree gives its groups no memory controller, so no cap can hold the mutants."
  exit 1
fi
sudo mkdir -p "$GROUP"
echo "$MEMORY_CAP" | sudo tee "${GROUP}/memory.max" > /dev/null
echo 0 | sudo tee "${GROUP}/memory.swap.max" > /dev/null
echo "$$" | sudo tee "${GROUP}/cgroup.procs" > /dev/null
if [[ "$(cat /proc/self/cgroup)" != "0::/${GROUP#/sys/fs/cgroup/}" ]] || [[ "$(cat "${GROUP}/memory.max")" != "$MEMORY_CAP" ]]; then
  echo "::error::The mutants run is not held in ${GROUP} at ${MEMORY_CAP} bytes."
  exit 1
fi
echo "The mutants run is held in ${GROUP}: memory.max $(cat "${GROUP}/memory.max"), memory.swap.max $(cat "${GROUP}/memory.swap.max")."

mebibytes() {
  local bytes=$1
  echo $((bytes / 1024 / 1024))
}

killed() {
  awk '$1 == "oom_kill" { print $2 }' "${GROUP}/memory.events"
}

held() {
  local kind=$1
  awk -v kind="$kind" '$1 == kind { print $2 }' "${GROUP}/memory.stat"
}

running() {
  local log name
  for log in "${OUT}"/log/*.log; do
    [[ -e "$log" ]] || continue
    name=$(sed -n '/^\*\*\* /{s///p;q}' "$log")
    if [[ -n "$name" ]] && ! grep -qxF "$name" "${OUT}"/caught.txt "${OUT}"/missed.txt "${OUT}"/unviable.txt "${OUT}"/timeout.txt 2> /dev/null; then
      echo "$name"
    fi
  done
}

report() {
  local free=$1 anon=$2
  echo "memory: anonymous $(mebibytes "$anon") MiB now, $(mebibytes "$(cat "$MOST_ANON")") MiB the most seen; page cache $(mebibytes "$(held file)") MiB; cap $(mebibytes "$MEMORY_CAP") MiB; processes killed at the cap $(killed); disk free $((free / 1024)) MiB; processes $(cat "${GROUP}/pids.current" 2> /dev/null || echo uncounted)"
}

watch() {
  local mutants=$1
  local -A started=()
  local tick=0 log name free oom seen_oom anon most=0
  seen_oom=$(killed)
  while sleep "$TICK_SECONDS"; do
    for log in "${OUT}"/log/*.log; do
      [[ -e "$log" ]] && [[ -z "${started[$log]:-}" ]] || continue
      name=$(sed -n '/^\*\*\* /{s///p;q}' "$log")
      [[ -n "$name" ]] || continue
      started[$log]=1
      echo "started  ${name}"
    done
    free=$(df --output=avail -k / | tail -n 1 | tr -d ' ')
    anon=$(held anon)
    if [[ "$anon" -gt "$most" ]]; then
      most=$anon
      echo "$most" > "$MOST_ANON"
    fi
    oom=$(killed)
    if [[ "$oom" != "$seen_oom" ]]; then
      seen_oom=$oom
      echo "::warning::The memory cap killed a process while these mutants ran: $(running | paste -sd ';' -)"
    fi
    tick=$((tick + 1))
    if [[ $((tick % TICKS_A_REPORT)) -eq 0 ]]; then
      report "$free" "$anon"
    fi
    if [[ "$free" -lt "$DISK_FLOOR_KIB" ]]; then
      echo "::error::The disk has ${free} KiB free, under ${DISK_FLOOR_KIB}, so the run stops; these mutants were running: $(running | paste -sd ';' -)"
      kill -TERM "$mutants"
      return
    fi
  done
}

held anon > "$MOST_ANON"
cargo mutants "${mutated[@]}" --no-shuffle --jobs 2 --shard "${SHARD}/${SHARDS}" --sharding round-robin \
  --caught --unviable --build-timeout-multiplier "$BUILD_TIMEOUT_MULTIPLIER" \
  "${excluded[@]}" "${examined[@]}" "${scope[@]}" &
mutants=$!
watch "$mutants" &
watcher=$!
status=0
wait "$mutants" || status=$?
kill "$watcher" 2> /dev/null || true
report "$(df --output=avail -k / | tail -n 1 | tr -d ' ')" "$(held anon)"
if [[ "$(killed)" != 0 ]]; then
  sudo dmesg | grep -i 'killed process' || true
fi
exit "$status"
