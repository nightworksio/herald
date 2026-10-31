# ADR-0008: Every line is covered, no mutant survives, and every parser is fuzzed

**Status:** Accepted
**Date:** 2026-10-31

## Context

herald posts in public, as somebody else's project. Its bugs land in front of
that project's community: a duplicate, a broken thread, a ping to everyone. It
runs on platforms its maintainers do not watch, so nobody may be looking when
something goes wrong.

NightWorksIO's other repositories already hold themselves to a bar.
lemonfiber, in Rust, covers every line and pins its toolchain.
php-mutation-gate gates itself at a mutation floor of 100. herald takes the
same bar, and adds what its own risks need: parsers that face the internet,
destinations it cannot call in CI, and stores it cannot all run anywhere but
on their platform.

## Decision

1. **The toolchain is pinned, and the oldest supported one is tested.**
   - **Pinned.** `rust-toolchain.toml` pins the toolchain every job and every
     clone builds with, and a bump is a reviewed commit.
   - **The MSRV.** The workspace is edition 2024 with `rust-version = "1.95"`,
     and the `msrv` job builds and tests on 1.95.
   - **wasm.** `herald-core` and the crates the wasm drivers use are built for
     `wasm32-unknown-unknown` in CI (ADR-0001).

2. **The lints refuse rather than warn.**
   - **The lint levels.** The workspace's lints:
     - forbid `unsafe_code`;
     - deny `missing_docs`;
     - turn on clippy's `pedantic` group;
     - deny `unwrap_used`, `expect_used`, `panic`, `indexing_slicing`, `todo`
       and `unimplemented`.
   - **In CI.** CI runs `cargo clippy --workspace --all-targets -- -D
     warnings`, so a pedantic warning fails the build.
   - **Tests.** `clippy.toml` allows `unwrap`, `expect`, `panic` and indexing
     in tests, and nowhere else.
   - **Formatting.** `rustfmt` is checked.
   - **The architecture.** The arch test and the disallowed-methods lists of
     ADR-0001 run with the suite.

3. **Every line is covered.**
   - **The gate.** `cargo llvm-cov` over `cargo nextest` fails under 100% line
     coverage, across the workspace.
   - **What it excludes.** It excludes only the serverless entry points that
     run on their platform alone: the Worker's, the Lambda's, Vercel's,
     Netlify's and Deno's. They are listed by path in one place, and each
     driver's smoke test runs them instead (decision 7).

4. **No mutant survives.** Mutation testing is cargo-mutants, and it runs in
   CI only, cut into shards that run side by side.
   - **What a run mutates.** A pull request mutates what its diff touches,
     with `--in-diff` against its merge base. `main` and a weekly scheduled
     run mutate the whole workspace.
   - **What fails it.** Any missed mutant fails the run. A mutant that does not
     build is not counted.
   - **Skips.** A skip is written in `.cargo/mutants.toml`, with the reason
     beside it, and nowhere else.

5. **Every inbound parser is fuzzed, and the deciding steps are
   property-tested.**
   - **The fuzz targets.** cargo-fuzz targets cover every parser that reads
     input herald did not write:
     - forge payloads and CI event files;
     - registry responses and feeds;
     - app events;
     - Discord and Slack interactions;
     - Matrix transactions;
     - ActivityPub activities and HTTP signature headers;
     - the config.
   - **Where the harness lives.** The harness is its own workspace under
     `fuzz/`, on nightly, so it inherits none of the lints that forbid
     panics, since reaching a panic is its purpose.
   - **When it runs.** Each pull request that touches a parser fuzzes that
     parser's targets for a bounded time. A nightly run fuzzes every target
     longer, with its corpus kept between runs.
   - **Property tests.** proptest holds the core's deciding steps:
     - chunking: no part over its limit, nothing lost, no grapheme or code
       fence broken;
     - routing: every matching route and only those;
     - the plan: a step is never planned twice, and none is ever dropped.

6. **Each destination is tested against recorded fixtures.**
   - **Recording.** A fixture is one request and its response, recorded from
     the live API by the recorder in `herald-testing`. The recorder strips
     credentials and identifying values before it writes.
   - **Replay.** In CI the replaying `Http` serves the fixtures, and any
     request the fixtures do not hold fails the test. A change to what herald
     sends therefore shows in the diff of the fixtures it needs.

7. **Each store passes the shared conformance suite, and each serverless
   driver has a smoke test.**
   - **Stores.** The conformance suite runs against every store in CI, on its
     real backend or its platform's local emulator (ADR-0005).
   - **Serverless drivers.** Each serverless driver is started in its
     platform's local runtime: the Worker in `workerd`, the Lambda in its
     emulator, and Deno, Vercel's and Netlify's entry points in theirs. The
     smoke test announces an app event through it, against recorded
     destinations.

8. **Docs and hygiene are checked like code.**
   - **The design.** ADRs are in `.docs/decisions/`. The guide and the
     reference are in `.docs/`, and prose states current fact.
   - **Hygiene.** CI runs typos, lychee, markdownlint, actionlint and zizmor
     over the whole tree.
   - **The line cap.** No tracked file is longer than 1000 lines, lockfiles
     and images aside, which a check of this repository's own enforces.

9. **Commits and merges.**
   - **Subjects.** Every commit subject and pull-request title is
     conventional: `type(scope): subject`.
   - **Signing.** Commits are signed.
   - **Merges.** `main` takes squash merges only. The squash commit's subject
     is the pull request's title and its message is the body, which ends
     with `Signed-off-by:`.
   - **Required checks.** Every check in CI is required on `main`, except
     Scorecard, which reports.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **A coverage floor below 100** | A floor below 100 says which lines may go untested without saying which, and the uncovered ones gather where testing is hardest: the error paths. Those are the paths that post twice or not at all. |
| **Mutation testing that reports and does not fail** | A report nobody must act on is soon read by nobody. A survivor is a test that would let a bug through, and it is fixed while the change is fresh. |
| **Mutating the whole workspace on every pull request** | It costs hours that a diff-scoped run does not, for mutants the change did not touch. `main` and the weekly run still mutate everything, so nothing goes unmutated for long. |
| **Live destination calls in CI** | They need real credentials in CI, they post to real channels, and they fail when a platform is slow. Recorded fixtures are deterministic, and a re-recording shows the platform's change as a diff. |
| **Fuzzing with the workspace's lints** | The lints forbid the panics a fuzzer exists to reach. The harness has its own workspace and toolchain, as lemonfiber's does. |

## Consequences

**A change arrives tested.** It cannot lower coverage, it cannot leave a
mutant alive in what it touched, and it cannot change a request without a
fixture saying so.

**The parsers that face the internet are the most tested code herald has.**

**CI is the gate, and it is long.** The full suite, coverage, mutation and
fuzzing run in CI. Locally, contributors run formatting, clippy and the tests
of what they changed.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): the boundaries the arch test holds
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the store conformance suite
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): the supply-chain checks in CI
- [ADR-0010](0010-herald-announces-itself-with-itself.md): herald's Action in its own CI
