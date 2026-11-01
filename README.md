<p align="center">
  <img alt="herald: a pixel-art crab blowing a golden trumpet beside the word herald" src="https://raw.githubusercontent.com/nightworksio/herald/main/.docs/brand/herald.svg" width="288">
</p>

<p align="center">
  <strong>Announce every release, version and feed entry to Discord, Matrix, Slack and the Fediverse, from one <code>herald.toml</code>.</strong>
</p>

<p align="center">
  <a href="https://github.com/nightworksio/herald/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/nightworksio/herald/actions/workflows/ci.yml/badge.svg?branch=main"></a>
  <a href="https://github.com/nightworksio/herald/actions/workflows/codeql.yml"><img alt="CodeQL" src="https://github.com/nightworksio/herald/actions/workflows/codeql.yml/badge.svg?branch=main"></a>
  <a href="https://scorecard.dev/viewer/?uri=github.com/nightworksio/herald"><img alt="OpenSSF Scorecard" src="https://api.securityscorecards.dev/projects/github.com/nightworksio/herald/badge"></a>
  <a href="#licence"><img alt="Licence: MIT OR Apache-2.0" src="https://img.shields.io/badge/licence-MIT%20OR%20Apache--2.0-blue"></a>
  <a href="Cargo.toml"><img alt="MSRV" src="https://img.shields.io/badge/dynamic/toml?url=https%3A%2F%2Fraw.githubusercontent.com%2Fnightworksio%2Fherald%2Fmain%2FCargo.toml&amp;query=%24.workspace.package%5B'rust-version'%5D&amp;label=MSRV"></a>
</p>

herald announces what happened to the places people gather. Each post is
formatted for its destination and threaded where the destination can thread,
and it is never posted twice. One `herald.toml` describes it all. The same
file runs as a GitHub Action, a serverless endpoint or a container, and
nothing long-lived is needed: a repository with a workflow and one secret is
enough to start.

> **In development.** No version is tagged, and herald is published to no
> registry. The first release is 0.1.0, tagged when every source,
> destination, store and layer below is built. The [status](#status) table
> says what is built today.

## What it does

**Four sources.**

- forge releases from GitHub, GitLab and Forgejo or Gitea;
- package versions from Packagist, crates.io, npm and Docker Hub;
- entries in RSS and Atom feeds;
- anything an app says, through the CLI or the HTTP API.

**Four destinations.**

- **Discord:** announcement, forum and text channels, with crossposting, a
  thread per announcement, and opt-in role pings pinned by `allowed_mentions`;
- **Matrix:** threaded messages in any room, from a bot account;
- **Slack:** Block Kit messages with threaded overflow, from a bot or an
  incoming webhook;
- **the Fediverse:** posts from a Mastodon-API account, or from herald's own
  ActivityPub actor that people follow per project.

**Three layers**, each serving everything the one before it does:

| Layer | Runs as | Adds |
|-------|---------|------|
| Actions-only | the CLI in GitHub Actions | announcements from forge events, scheduled polling and thread housekeeping |
| Serverless | Cloudflare Workers, AWS Lambda, Vercel, Netlify or Deno Deploy | the HTTP API, verified webhooks, slash commands and buttons, Matrix commands, the ActivityPub actor |
| Container | a signed multi-arch image on GHCR | the same endpoint, with SQLite on a volume or Postgres |

`herald doctor` names every feature a config asks for that the layer it runs
in cannot serve, before anything posts.

**Never twice, never half-posted.** Each announcement has a deterministic
key, and every step that lands is recorded: the post, the crosspost, the
thread, each overflow chunk and the pin. A rerun finds the key and sends
nothing. An interrupted run is finished by the next one, which sends only
what is missing.

## Quick start

The snippets below show the configuration that
[ADR-0006](.docs/decisions/0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md)
and
[ADR-0009](.docs/decisions/0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md)
fix. Neither the CLI nor the Action is built yet, so they do not run.

`herald.toml`, announcing php-mutation-gate's releases into a Discord
announcement channel:

```toml
#:schema https://raw.githubusercontent.com/nightworksio/herald/main/herald.schema.json
version = 1

[store]
kind = "git-branch"

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[sources.php-mutation-gate]
kind = "github"
repository = "nightworksio/php-mutation-gate"

[[routes]]
id = "php-mutation-gate-releases"
sources = ["php-mutation-gate"]
kinds = ["release"]
destination = "discord"
channel = "1566184281406177531"
template = "package-release"
crosspost = true
thread = { name = "{{ event.project }} {{ event.version }}", archive_after = "14d" }
mentions = { roles = ["<the php-mutation-gate role's id>"] }

[templates.package-release]
title = "📦 {{ event.project }} {{ event.version }}{% if event.prerelease %} · pre-release{% endif %}"
body = "{{ event.body }}"
```

`.github/workflows/announce.yml`, with the Action pinned by commit:

```yaml
on:
  release:
    types: [published]

permissions:
  contents: write

jobs:
  announce:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: nightworksio/herald@<commit-sha>
        env:
          HERALD_DISCORD_TOKEN: ${{ secrets.HERALD_DISCORD_TOKEN }}
```

`contents: write` is for the `git-branch` store, which keeps herald's
never-twice ledger on a `herald-state` branch.

## Status

What is built on `main`, and the decision each part follows:

| Part | Decided in | Built |
|------|-----------|-------|
| The workspace, its layering and CI | [ADR-0001](.docs/decisions/0001-a-sans-io-core-behind-ports.md), [ADR-0008](.docs/decisions/0008-every-line-is-covered-and-no-mutant-survives.md) | yes |
| The core: events, routes, rendering, dialects, chunking, the never-twice plan | [ADR-0001](.docs/decisions/0001-a-sans-io-core-behind-ports.md) | yes |
| The ports, fakes and the store conformance suite | [ADR-0001](.docs/decisions/0001-a-sans-io-core-behind-ports.md), [ADR-0005](.docs/decisions/0005-an-announcement-is-sent-once-and-finished-if-interrupted.md) | yes |
| `herald.toml` and its JSON Schema | [ADR-0006](.docs/decisions/0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md) | yes |
| Sources: forges, registries, feeds, apps | [ADR-0003](.docs/decisions/0003-every-source-becomes-one-neutral-event.md) | forge releases from GitHub, GitLab and Forgejo, by their event and by polling; package versions from Packagist, crates.io, npm and Docker Hub, by polling; feeds and apps no |
| Destination: Discord | [ADR-0004](.docs/decisions/0004-each-destination-gets-its-own-dialect-limits-and-features.md), [ADR-0011](.docs/decisions/0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) | yes, against reference fixtures until recorded ones replace them |
| Destinations: Matrix, Slack, Fediverse | [ADR-0004](.docs/decisions/0004-each-destination-gets-its-own-dialect-limits-and-features.md) | no |
| Stores: SQLite, Postgres, D1, DynamoDB, Deno KV, markers, git branch, Actions cache, repository variables | [ADR-0005](.docs/decisions/0005-an-announcement-is-sent-once-and-finished-if-interrupted.md) | no |
| The CLI, `herald doctor` and the Action | [ADR-0002](.docs/decisions/0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md), [ADR-0009](.docs/decisions/0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md) | no |
| Serverless drivers and the container | [ADR-0002](.docs/decisions/0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md), [ADR-0009](.docs/decisions/0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md) | no |
| herald announcing its own releases | [ADR-0010](.docs/decisions/0010-herald-announces-itself-with-itself.md) | no |

## How it is designed

The design is recorded as decisions in [`.docs/decisions`](.docs/decisions/README.md).
Start with [ADR-0001](.docs/decisions/0001-a-sans-io-core-behind-ports.md):
a sans-IO core behind ports, so the same code that decides what to post runs
on a GitHub runner, in a Worker and in a container.

The bar is high and checked by CI on every change: 100% line coverage, no
surviving mutant, fuzzing of every parser, clippy pedantic as errors, and
every action pinned by SHA
([ADR-0008](.docs/decisions/0008-every-line-is-covered-and-no-mutant-survives.md)).

## Security

Secrets are named in the config and never written in it. They are redacted
in every log, error and dry run, and every inbound request is verified
before it is parsed
([ADR-0007](.docs/decisions/0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md)).
Report a vulnerability privately, as [SECURITY.md](SECURITY.md) says.

## Contributing

[CONTRIBUTING.md](CONTRIBUTING.md) says what a contribution needs and how to
check one locally. Everyone here keeps the
[code of conduct](CODE_OF_CONDUCT.md).

## Licence

herald is licensed under either of

- the [Apache License, Version 2.0](LICENSE-APACHE), or
- the [MIT licence](LICENSE-MIT),

at your option. Copyright NightWorksIO.

Unless you explicitly state otherwise, any contribution you intentionally
submit for inclusion in herald, as defined in the Apache-2.0 licence, is
dual-licensed as above, without any additional terms or conditions.

The logo and its generator are in [`.docs/brand`](.docs/brand).
