# ADR-0010: herald announces itself with itself

**Status:** Accepted
**Date:** 2026-10-31

## Context

php-mutation-gate holds itself to the gate it ships. A tool that asks its
users to trust it with their announcements should show the same: its own
releases announced by its own binary, through its own Action, from its own
config. If herald's Action broke, herald's own release would be the first to
find out, not a user's.

herald's first real user is php-mutation-gate. Its releases go into the
NightWorksIO Discord announcement channel `package-releases`:

- each post is crossposted;
- each release gets a thread of its own;
- each post has a package-first title, such as "📦 php-mutation-gate v0.1.0";
- each post pings an opt-in role for its package;
- a pre-release is marked as one.

herald's own releases go to the same channel, in the same form.

## Decision

1. **herald's repository carries its own `herald.toml`, which is the
   documented example.**
   - **What it routes.** It routes herald's releases and pre-releases into
     `package-releases` (channel `1566184281406177531`) as the first user's
     config does (ADR-0006): crossposted, threaded, package-first, pinging
     herald's own opt-in role, with pre-releases marked.
   - **Where the example comes from.** The guide's example is this file. A
     test holds the guide's copy to it, so the documented example is always
     one that runs.
   - **Checked in CI.** CI runs `herald doctor --layer actions` on it, so the
     config, its templates and the schema are checked on every change.

2. **A release announces itself with the binary built from its tag.**
   - **How.** The release workflow builds and attests the binaries, publishes
     the release, and then runs herald's own `action.yml` at the tagged
     commit. The Action downloads the binary that release just published and
     checks its checksum (ADR-0009).
   - **What it announces.** It announces the release into `package-releases`,
     marked as a pre-release where it is one.
   - **A failed announcement.** A release whose own announcement fails is a
     failed release workflow.

3. **CI runs herald's own Action from the pull request's commit.**
   - **How.** On every pull request, a job runs `uses: ./` in Actions-only
     mode, so the Action builds the CLI from that commit (ADR-0009). The CLI
     announces a sample release through a config whose destinations point at
     CI's recorded-fixture server (ADR-0004), on the `markers` and
     `actions-cache` stores.
   - **What it proves.** `action.yml`, the CLI, the config schema and the
     never-twice key are exercised as a user runs them, and a second run of
     the same event must send nothing.

4. **herald's own release threads are kept by herald.** A scheduled workflow
   in this repository runs `herald housekeep` with the same config, archiving
   herald's release threads after the config's `archive_after` and pinning
   the newest release.

5. **php-mutation-gate consumes herald pinned by commit until herald's first
   tag.** Its workflow calls herald's Action at a commit SHA, and the Action
   builds that commit (ADR-0009). Its `herald.toml` is the first user's config
   of ADR-0006, with the package's opt-in role.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **Announce herald's releases with a separate script** | The script would be tested, and the product would not. The release that matters most to herald would then go out by a path no user takes. |
| **Announce with the previous release's binary** | A broken Action or CLI in the new release would be announced by a working old one, and the breakage would reach users first. |
| **A failed announcement as a warning** | A release whose announcement fails is a release whose users were told nothing. It is also proof that the Action is broken. Failing the workflow makes it the first thing fixed. |
| **Dogfooding against the live channel on every pull request** | Every pull request would post to a real community. The fixture server exercises the same Action, CLI and config without a post. |

## Consequences

**Every change to herald is exercised by herald before it ships.** The
Action, the CLI and the config schema run on every pull request, and the
release runs them for real.

**The documented example always runs.** It is the config herald's own
releases use.

**A broken release cannot go quietly.** If herald cannot announce itself, the
release workflow is red.

## Related

- [ADR-0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md): Actions-only, the layer the dogfooding runs on
- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): crossposts, threads, roles and the fixture server's base URL
- [ADR-0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md): the first user's config
- [ADR-0009](0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md): the Action's download and build paths
