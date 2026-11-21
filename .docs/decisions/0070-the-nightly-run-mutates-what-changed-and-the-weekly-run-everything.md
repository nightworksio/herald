# ADR-0070: The nightly run mutates what changed, and the weekly run everything

**Status:** Accepted
**Date:** 2026-11-21

## Context

ADR-0008 mutates the whole workspace in a nightly scheduled run on `main`.
The first such run took 1,284 runner-minutes: twelve shards of 82 to 131
minutes each, all at once, on a plan whose twenty concurrent jobs every
repository of the organisation shares. Most nights, most crates have not
changed since the night before. The user decided: the nightly run mutates
only the crates changed since the last green nightly run, and every crate
depending on one; the whole workspace is mutated weekly; and a scheduled
run's shards run at most four at a time.

## Decision

1. **Two schedules.** `ci` runs nightly at 02:17 UTC from Sunday to Friday,
   and weekly at 02:17 UTC on Saturday. The cron a scheduled run started by
   tells the two apart.
2. **What each run mutates.** One job, `mutants-scope`, decides it through
   `.github/scripts/mutated.py`, and every mutants job reads its outputs:
   - **A pull request** mutates its diff (ADR-0008).
   - **The weekly run, a run started by hand and a release's gate** mutate
     the whole workspace.
   - **The nightly run** mutates the crates changed since the head of the
     last green scheduled run on `main`, nightly or weekly, and every
     workspace member depending on one of them, by any kind of dependency,
     directly or through another. A crate's own directory holds its
     changes. The dependencies are read with `cargo metadata --no-deps`,
     and the changes with `git diff --name-only`.
   - **A push to `main`** mutates nothing, as before.
   - **Every event.** `mutants-scope` runs on every pull request, push,
     scheduled run, run started by hand and release gate, so it reports a
     check on each and can be required; where it names nothing, the jobs
     it skips pass.
3. **When the nightly run mutates the whole workspace.**
   - **No green run.** When no green scheduled run on `main` is listed, or
     this clone does not hold its commit, or the runs cannot be listed.
   - **A change outside the crates.** When a path outside every crate
     changed, unless it is one that no build and no test reads: the ADRs,
     the other workflows and scripts under `.github/` (but not `ci.yml`,
     `install-tool.sh` or the dogfood config), the fuzz harness, and the
     licences and hygiene configs. The script names each of these, and its
     test refuses a crate source that names one of them as a path.
4. **Nothing changed.** When the nightly run finds no changed crate,
   `mutants-scope` says so, the shards and the runtime jobs are skipped,
   and `mutants` passes.
5. **The runtime jobs.** `mutants-wasm`, which mutates `herald-wasm`'s
   JavaScript glue, runs where `herald-wasm` is in the scope; the shards
   mutate the rest of it. `mutants-compute` mutates `herald-wasi` and
   `herald-fastly` only where each is in the scope; the shards exclude
   both.
6. **Four shards at a time.** A scheduled run's twelve shards run four at
   a time (`max-parallel`). A pull request's are not capped.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **The whole workspace every night** | 1,284 runner-minutes a night, most of it for crates nothing changed in, while the shards hold most of the organisation's runners. |
| **Only the crates changed, without their dependents** | A change to a crate changes what its dependents' code does, and their mutants are killed or survive by it. |
| **Every path outside the crates mutating the whole workspace** | The ADRs change nearly every day, and no build or test reads them, so nearly every nightly run would mutate everything. |
| **Since the last nightly run, green or not** | A red run's surviving mutants would not be mutated again until the weekly run. Comparing with the last green run mutates them again each night until they are killed. |
