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
depending on one; the whole workspace is mutated weekly; a scheduled
run's shards run at most four at a time; and a run's shards are as many as
its mutants take, so none runs past its limit and a small change takes
few.

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
6. **Shards, as many as the mutants take.** `mutants-scope` lists the
   mutants the shards mutate, with `cargo mutants --list` and the shards'
   own arguments, a pull request's narrowed to its diff, and cuts as many
   shards as keep each inside its aim at the slowest rate seen, after four
   minutes of setup:
   - **Every run but a pull request's:** 8 seconds a mutant, a shard
     aiming at 90 minutes, half its 180-minute limit, and at most 40
     shards; a scheduled run's run four at a time (`max-parallel`), any
     other's twelve.
   - **A pull request:** 30 seconds a mutant, a shard aiming at 15
     minutes, and at most 12 shards, all run at once; a small change runs
     one.
   Where the mutants cannot be listed, the run takes the most shards;
   where none is listed, no shard runs and `mutants` passes.
7. **Where the rates come from.** Each shard's seconds from
   cargo-mutants' `Found` line to its summary, over its mutants, read from
   the job logs:
   - **Every run but a pull request's:** the scheduled runs 44891794888
     (2026-11-21; 4.10 to 6.59 seconds a mutant), 45008931805 (2026-11-22;
     4.51 to 7.62), 45393470118 (2026-11-24; 5.49 to 7.64, and one shard
     cancelled at 180 minutes, so slower than 7.72), 45602562982
     (2026-11-25; three shards 6.11 to 6.72, nine cancelled, slower than
     7.18) and 45805054961 (2026-11-26; eleven shards cancelled, slower
     than 7.17). The slowest, 7.72, rounds up to 8.
   - **A pull request:** the 101 shards with a mutant of the pull request
     runs 45530478612, 45557289174, 45800494724, 45852366772, 45859174772,
     45869363167, 45870522580, 45908553090 and 45910907079. A shard takes
     about 155 seconds of baseline and then its mutants; past 180 seconds
     of baseline a shard, the slowest run's mutants, 45800494724's, took
     30.3 seconds each, so 30.
   - **Setup:** at most 0.8 minutes from a shard's start to cargo-mutants'
     `Found` line, in every shard above, and a baseline build and test of
     153 to 179 seconds in the shards whose baseline was read (shard 0 of
     44891794888, 45910907079 and 45557289174), so four minutes.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **The whole workspace every night** | 1,284 runner-minutes a night, most of it for crates nothing changed in, while the shards hold most of the organisation's runners. |
| **Only the crates changed, without their dependents** | A change to a crate changes what its dependents' code does, and their mutants are killed or survive by it. |
| **Every path outside the crates mutating the whole workspace** | The ADRs change nearly every day, and no build or test reads them, so nearly every nightly run would mutate everything. |
| **Since the last nightly run, green or not** | A red run's surviving mutants would not be mutated again until the weekly run. Comparing with the last green run mutates them again each night until they are killed. |
