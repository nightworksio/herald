# ADR-0012: A release is drafted by the bot, signed by the maintainer, and published after the full gate

**Status:** Accepted
**Date:** 2026-11-01

## Context

ADR-0009 says what a release holds: a binary for each of six targets, one
`SHA256SUMS`, an attestation per binary, the Action, the image and the wasm
build. ADR-0010 says a release announces itself. Neither says who cuts a
release, how the version is chosen, where its notes come from, or what is
checked before anything is published.

A release is the one act that reaches every consumer at once. The version
should follow the commits, the notes should be written once and edited by a
person, and nothing should be published that the maintainer did not sign or
that the gate did not pass. NightWorksIO's other package, php-mutation-gate,
releases this way, and herald follows it.

## Decision

1. **A release is drafted by the bot.**
   - **What starts it.** The maintainer runs `release.yml` by
     `workflow_dispatch`, with a version or without.
   - **The version.** Without one, it is the last tag moved by the commits
     since: `feat` is a minor, `fix` and `perf` a patch, and `!` or a
     `BREAKING CHANGE:` footer a major, or a minor while the major is 0. The
     first version, `1.0.0` and any pre-release such as `0.1.0-rc.1` are
     given by hand.
   - **The notes.** git-cliff, pinned by version and checksum, writes the
     version's section of `CHANGELOG.md` from `main`'s squash commits.
   - **The pull request.** The App opens `release/vX.Y.Z` with that section,
     through `createCommitOnBranch` with no custom author, so GitHub signs the
     commit and CI runs on it. A version whose branch exists is refused, so
     edits made there are never discarded.
   - **The App.** It holds Contents and Pull requests, read and write, on
     this repository. Its private key is the secret `HERALD_BOT_PRIVATE_KEY`
     of the environment `bot`, whose deployment-branch policy allows `main`
     only, and its client id is the repository variable
     `HERALD_BOT_CLIENT_ID`. Until both are set, the drafting job says so and
     skips.

2. **The maintainer signs it.** The maintainer edits the section in the pull
   request, merges it, runs `git tag -s vX.Y.Z` on the squash commit and
   pushes the tag. A tag ruleset lets only the maintainer create `v*` tags.

3. **A tag is published only after the full gate.** On the tag's push,
   `release.yml`:
   - **verifies the tag.** Its signature is checked against
     `.github/allowed_signers` as `main` holds it, never as the tag holds it,
     and a repository without the file publishes nothing. A lightweight tag,
     or one on a commit that is not on `main`, is refused.
   - **runs the full gate.** `ci.yml` runs on the tagged commit, mutating the
     whole workspace (ADR-0008).
   - **builds and attests.** Each of the six targets builds with
     `cargo build --locked --release`, into `herald-<target>.tar.gz`, or
     `.zip` on Windows. One `SHA256SUMS` lists every archive, and each
     archive carries its build provenance attestation (ADR-0009).
   - **publishes.** In the environment `release`, whose deployment policy
     allows `v*` tags only, and only while the tag still points at the commit
     the gate passed, the release takes the version's section as its notes
     and the archives and `SHA256SUMS` as its assets. A pre-release is marked
     as one.
   - **moves the release line.** The tag of the version's line moves to it:
     `vMAJOR.MINOR` while the major is 0 (`v0.1`), and `vMAJOR` from 1.0.0. A
     pre-release moves no line.
   - **announces itself.** It runs `uses: ./` at the tag, with the Action's
     `release` input naming the release, so the Action downloads the binary
     just published and checks it, and its `tag` input naming it, so
     `herald announce --from github --release vX.Y.Z` announces it
     (ADR-0010). A failed announcement fails the workflow.

4. **The Action runs a release line's newest release.** Called at a line's
   tag, such as `@v0.1`, the Action resolves it to the newest release on that
   line, leaving out drafts and pre-releases, and downloads and checks that
   release as it does at a version's tag. This supersedes in part ADR-0009,
   decision 2, under which every ref other than a version's tag built from
   source.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **release-please** | The bot would create the tag, unsigned, and a release must be the maintainer's signed act. |
| **release-drafter** | It needs labels, its autolabeler runs under `pull_request_target`, and it ignores conventional commits. |
| **Any pushed `vX.Y.Z` tag publishes** | Anyone who can push a tag could publish, and nothing ties the release to the maintainer. |
| **A signed tag with no drafting** | The version and the notes would be chosen and written by hand for every release, and drift from the commits. |
| **`GITHUB_TOKEN` opens the release pull request** | CI does not run on its commits, so every release pull request would wait for a maintainer to start it. |
| **A personal access token** | The bot would act as the maintainer, with a long-lived credential and the maintainer's reach. |
| **Announcing from a `release: published` workflow** | A release published with `GITHUB_TOKEN` starts no workflow, and a second workflow could announce a release whose gate it never saw. |

## Consequences

**A release is the maintainer's signed act**, verified before anything is
published, and published only from a commit the full gate passed.

**The version and the notes follow the commits**, written once by the bot and
edited by a person in a pull request.

**`@v0.1` downloads a checked binary.** A consumer pinned to a line gets its
fixes without rebuilding from source.

**Three settings are made once, by the maintainer:** the App with its key and
client id, the environments `bot` and `release`, and the tag ruleset.

## Related

- [ADR-0008](0008-every-line-is-covered-and-no-mutant-survives.md): the gate a release passes
- [ADR-0009](0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md): what a release holds; decision 2 is superseded in part
- [ADR-0010](0010-herald-announces-itself-with-itself.md): the release announcing itself
