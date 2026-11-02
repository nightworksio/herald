# ADR-0009: herald ships as signed binaries, an Action, an image and a wasm build, from GitHub alone until 1.0

**Status:** Accepted; decision 2 superseded in part by [ADR-0012](0012-a-release-is-drafted-by-the-bot-signed-by-the-maintainer-and-published-after-the-full-gate.md)
**Date:** 2026-10-31

## Context

herald runs on GitHub's runners, on four serverless platforms and in a
container (ADR-0002). Each wants a different artefact: a native binary, an
Action that fetches it, an image, or a wasm module. Each artefact runs with
its user's secrets in reach, so each must be verifiable (ADR-0007).

Until 1.0 the interfaces still move. Publishing to crates.io, npm and Docker
Hub would hand out names whose meaning could change under the people who
took them. And nothing is released before herald is complete: every source,
destination, store and layer is built before the first tag.

## Decision

1. **CLI binaries for six targets, attached to each GitHub release.**
   - **The targets.** Linux (`x86_64` and `aarch64`, static musl), macOS
     (`x86_64` and `aarch64`) and Windows (`x86_64` and `aarch64`, MSVC).
   - **Each release.** Each release attaches every binary, one `SHA256SUMS`
     file, and the GitHub build provenance attestation of each.
   - **The build.** The release workflow is this repository's own, with every
     action pinned by SHA (ADR-0007).

2. **The Action ships from the same repository.** `action.yml` is a composite
   action, pinned by a user to a tag or a commit SHA. It reads the ref it was
   called at.
   - **At a release tag.** It downloads that release's binary for the runner,
     checks its SHA-256 against the release's `SHA256SUMS`, and runs it.
   - **At any other ref, such as a commit SHA.** It builds the CLI from its own
     checkout with `cargo build --locked`, caches the build by commit, and
     says in the log that it built from source.

   Either way it runs `herald announce --from github`, with inputs passed
   through the environment (ADR-0007). Building from source is what lets a
   project consume herald pinned by commit before the first tag, and herald's
   own CI runs that path on every pull request (ADR-0010).

3. **A container image on GHCR.**
   - **What it is.** `ghcr.io/nightworksio/herald` is multi-arch
     (`linux/amd64` and `linux/arm64`) and runs `herald-server` (ADR-0001) as
     a non-root user on a distroless static base, pinned by digest.
   - **Signing.** It is signed with cosign keyless, and carries build
     provenance and an SBOM.
   - **Configuration.** It reads `herald.toml` from a mounted path, keeps
     SQLite on a volume or uses Postgres, and reads secrets from mounted files
     or the environment.

4. **The wasm build, and the deploy templates.**
   - **The wasm build.** It serves Deno Deploy, Vercel and Netlify. Each
     release attaches it as an archive per platform: the module, the
     platform's thin entry point, and its attestation.
   - **The templates in `deploy/`.** They hold a ready project per serverless
     platform:
     - a `wrangler.toml` for the Worker, with D1 and cron;
     - a template for the Lambda, with DynamoDB and EventBridge Scheduler;
     - a project each for Vercel, Netlify and Deno Deploy, with their cron.

5. **Releases are signed keylessly** (ADR-0007). No signing key exists:
   - the attestations name the workflow and commit that built each artefact,
     which `gh attestation verify` checks;
   - `cosign verify` checks the image;
   - `SECURITY.md` says how to run both.

6. **Versions follow semantic versioning from the first tag.**
   - **The first tag.** It is `v0.1.0`. It is cut when every source,
     destination, store and layer in the ADRs is built, and not before.
   - **What semver protects.** It protects:
     - the CLI's commands, options and exit codes;
     - the config and its schema;
     - the app event schema;
     - the Action's inputs and outputs;
     - the HTTP API;
     - the public API of ADR-0001.
   - **Until then.** A consumer pins herald by commit, and the Action builds
     from source (decision 2).

7. **GitHub-hosted artefacts only, until 1.0.** Release binaries, the Action,
   the GHCR image and the wasm archives are all hosted on GitHub. crates.io,
   npm and Docker Hub follow at 1.0.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **cargo-dist for the release workflow** | It generates workflows that reference actions by tag, which the pinning rule and zizmor refuse, and every regeneration would undo a hand-pinned edit. A workflow of herald's own is pinned once and kept that way by Dependabot. |
| **A rolling `edge` pre-release from `main`** | It is a tag in all but name, before the first release, and it would make every commit on `main` an artefact users can come to depend on. Building from source at a pinned commit gives the same reach without publishing anything. |
| **Downloading `main`'s CI artifact by SHA** | Artifacts expire after 90 days, and downloading one needs a token with `actions: read` on herald's repository. |
| **A JavaScript action wrapping the binary** | It needs a Node runtime and a bundled `dist/`, a second build to keep in step. A composite action that downloads and verifies one binary is all the Action has to be. |
| **Publishing crates and packages now** | Names taken before 1.0 bind herald to interfaces that are still moving, and every registry is one more place a compromised release lands. GitHub alone keeps one place to verify until the interfaces settle. |
| **A Debian or Alpine base image** | A shell and a package manager in the image are tools for whoever breaks into it. herald needs neither, so distroless static carries only the binary and its certificates. |

## Consequences

**Every artefact is verifiable with GitHub's tooling.** No key has to be
fetched or trusted.

**A project can adopt herald before its first release.** It pins a commit, and
the Action builds that commit.

**The registries wait for 1.0.** Until then herald's names on crates.io, npm
and Docker Hub are unclaimed by herald.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): the drivers each artefact carries
- [ADR-0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md): the layers each artefact serves
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): signing and verification
- [ADR-0010](0010-herald-announces-itself-with-itself.md): the release that announces itself
- [ADR-0012](0012-a-release-is-drafted-by-the-bot-signed-by-the-maintainer-and-published-after-the-full-gate.md): how a release is drafted, signed and published, and the Action at a release line
