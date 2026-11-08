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
| Container | a signed multi-arch image on GHCR, under Compose, Kubernetes or Helm | the same endpoint, with SQLite on a volume or Postgres |

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
fix. No release of herald exists yet, so the Action has no binary to download.

`herald.toml`, herald's own, announcing its releases and pre-releases into
the NightWorksIO Discord's `package-releases` announcement channel:

```toml
#:schema https://raw.githubusercontent.com/nightworksio/herald/main/herald.schema.json
version = 1

[store]
kind = "markers"

[markers]
key = { secret = "HERALD_MARKER_SECRET" }

[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

[sources.herald]
kind = "github"
repository = "nightworksio/herald"

[[routes]]
id = "herald-releases"
sources = ["herald"]
kinds = ["release"]
destination = "discord"
channel = "1566184281406177531"
template = "package-release"
crosspost = true
thread = { name = "{{ event.project }} {{ event.version }}", archive_after = "14d" }
pin = "markers"

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
  contents: read

concurrency:
  group: herald

jobs:
  announce:
    runs-on: ubuntu-latest
    environment: announce
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1
        with:
          persist-credentials: false
      - uses: nightworksio/herald@<commit-sha>
        env:
          HERALD_DISCORD_TOKEN: ${{ secrets.HERALD_DISCORD_TOKEN }}
          HERALD_MARKER_SECRET: ${{ secrets.HERALD_MARKER_SECRET }}
```

The `markers` store keeps herald's never-twice ledger in the posts herald
makes, so the run writes nothing to the repository. It has no compare-and-set,
so the `concurrency` group keeps to one herald run at a time. `herald init`
prints a new `HERALD_MARKER_SECRET`, once.

### herald's own ActivityPub actor

A `kind = "activitypub"` destination announces as an actor on your own
domain, which people follow per project. Each route names its actor:

```toml
[destinations.fediverse]
kind = "activitypub"
domain = "herald.example.org"
key = { secret = "HERALD_ACTIVITYPUB_KEY" }

[[routes]]
id = "herald-notes"
sources = ["herald"]
destination = "fediverse"
actor = "herald"
template = "package-release"
```

The actor signs with an RSA private key of 2048 to 8192 bits in a PKCS#8
PEM. Make one with:

```sh
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 -out herald-activitypub.pem
```

Store the file's contents as `HERALD_ACTIVITYPUB_KEY`. Every run loads the
key before it sends anything. `herald doctor` loads it too, wherever it is
set.

People find and follow the actor through `herald server`.

### herald server

`herald server` answers forge webhooks and serves herald's ActivityPub
actors, and polls and keeps house on `[schedule]`, until it is stopped. It
listens on `127.0.0.1:8080`, for a reverse proxy in front of it that
terminates TLS:

```toml
[sources.herald]
kind = "github"
repository = "nightworksio/herald"
webhook = { secret = "HERALD_GITHUB_WEBHOOK" }

[schedule]
poll = "30m"
housekeep = "12h"

[server]
listen = "127.0.0.1:8080"
```

- Point the repository's release webhook at `https://<your host>/hooks/herald`, with the secret stored as `HERALD_GITHUB_WEBHOOK`.
- herald answers each verified release with 202 and announces it after the reply.
- An ActivityPub actor is served at `/users/<actor>`, with its Notes, its inbox, the shared inbox `/inbox` and WebFinger.
- `--listen` or `HERALD_LISTEN` takes the place of `listen`.
- `[server]` also sets `jobs`, `connections`, `body`, `fetches`, `completions` (how many autocompletions a session answers at once), `deliveries` and `silence` (how long a session hears nothing before it checks its connection), and `[server.keys]` sets where inbound activities' keys are kept (`cache = "memory"` or `"store"`), `refresh_after`, `reread_after` and `most` ([ADR-0014](.docs/decisions/0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md)).

`herald doctor --layer server` names what the config asks for that the
server cannot serve.

### An app's own events

Any app can announce through herald: a deploy script, a CI step, a service
of your own. A route names the app among its sources, and the app holds a
token herald keeps only the hash of:

```toml
[[routes]]
id = "deploys"
sources = ["ci"]
destination = "discord"
channel = "1566184281406177531"
template = "deploy"

[apps.ci]
tokens = ["sha256:…"]
projects = ["herald"]
```

- `herald token new --app ci` prints a new token once, with the `tokens` line to keep. Add a second token before removing the first to rotate it.
- An event is herald's event JSON, as [`herald-event.schema.json`](herald-event.schema.json) describes it: `{"id": "7", "project": "herald", "title": "Deployed"}`, with `version`, `body`, `links`, `prerelease` and `occurred` where there are any.
- `herald announce --event deploy.json --app ci` announces it from a file, or from standard input with `--event -`.
- `herald server` takes it at `POST /api/projects/herald/events` with `Authorization: Bearer <token>`: 202 for a new event, 200 for one it has already ([ADR-0015](.docs/decisions/0015-an-app-is-a-source-routes-name-announcing-with-hashed-tokens-scoped-to-its-projects.md)).
- `GET /api/projects/herald/events/7`, with the same token, answers where its announcement stands on each of the app's routes: `pending`, `announcing`, `announced`, or `stopped` with the steps that remain.

### Commands in your channels

Members ask herald in the channel it announces in: `subscribe` and
`unsubscribe` to be mentioned when a project announces, `latest` for what it
announced last, and `changelog` for its notes. Each announcement can carry a
button that subscribes.

```toml
[destinations.discord.interactions]
application = "1566100000000000000"
key = "<the application's public key>"

[[routes]]
id = "releases"
sources = ["herald"]
destination = "discord"
channel = "1566184281406177531"
template = "release"
subscribe = { role = "1566190000000000001" }
```

- Each destination takes its commands every way it offers, set by `via` ([ADR-0016](.docs/decisions/0016-commands-are-carried-every-way-a-destination-offers-and-answered-from-what-herald-keeps.md)):
  - Discord: `http`, posted to `/interactions/discord`, or `gateway`, held by `herald server`.
  - Slack: `http`, posted to `/interactions/<destination>`, or `socket`, Socket Mode held by `herald server`.
  - Matrix: `appservice`, pushed to `/matrix/<destination>`, or `sync`, as the bot.
- A gateway connects only under `discord.gg` and Socket Mode only under `slack.com`; a resume or Socket Mode address elsewhere is refused. `domains = ["proxy.example"]` in the interactions adds domains, such as a proxy's.
- `herald discord commands` registers the commands. `herald discord roles --guild <id>` lists a server's roles, and `--create <name>` makes an opt-in role.
- `herald slack manifest --url https://<your host>` prints the Slack app's manifest. `herald matrix registration --url https://<your host>` prints the homeserver's registration with two new tokens, once.
- On Discord, subscribing gives the route's opt-in role, which the route mentions. On Slack and Matrix, it keeps the member in the store, for a route with `mentions = { subscribers = true }`.
- A command names a project, or acts on the only one announced in its channel; Discord completes the names.
- Answers to `latest` and `changelog` are the asker's alone unless `replies = "public"` or the asker adds `share`. `commands`, `prereleases` and `template` set which commands are answered and what they show.
- `herald doctor --layer server` names a transport a layer cannot carry.

## Status

What is built on `main`, and the decision each part follows:

| Part | Decided in | Built |
|------|-----------|-------|
| The workspace, its layering and CI | [ADR-0001](.docs/decisions/0001-a-sans-io-core-behind-ports.md), [ADR-0008](.docs/decisions/0008-every-line-is-covered-and-no-mutant-survives.md) | yes |
| The core: events, routes, rendering, dialects, chunking, the never-twice plan | [ADR-0001](.docs/decisions/0001-a-sans-io-core-behind-ports.md) | yes |
| The ports, fakes and the store conformance suite | [ADR-0001](.docs/decisions/0001-a-sans-io-core-behind-ports.md), [ADR-0005](.docs/decisions/0005-an-announcement-is-sent-once-and-finished-if-interrupted.md) | yes |
| `herald.toml` and its JSON Schema | [ADR-0006](.docs/decisions/0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md) | yes |
| Sources: forges, registries, feeds, apps | [ADR-0003](.docs/decisions/0003-every-source-becomes-one-neutral-event.md) | forge releases from GitHub, GitLab and Forgejo, by their event and by polling; package versions from Packagist, crates.io, npm and Docker Hub, and RSS and Atom feed entries, by polling; an app's events, through `herald announce --event` and `herald server` ([ADR-0015](.docs/decisions/0015-an-app-is-a-source-routes-name-announcing-with-hashed-tokens-scoped-to-its-projects.md)) |
| Destination: Discord | [ADR-0004](.docs/decisions/0004-each-destination-gets-its-own-dialect-limits-and-features.md), [ADR-0011](.docs/decisions/0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) | yes, against reference fixtures until recorded ones replace them |
| Inbound requests verified before they are parsed | [ADR-0007](.docs/decisions/0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md) | the signatures of GitHub, GitLab and Forgejo webhooks, Discord interactions and Slack requests, with the replay window, and the homeserver token of a Matrix transaction, each webhook delivery admitted once ([ADR-0013](.docs/decisions/0013-a-webhook-delivery-is-admitted-once-released-when-it-fails-and-kept-for-a-week.md)), and an ActivityPub activity's HTTP signature and digest against its actor's key, read from a public address on the key's own origin, with the activity's actor held to that key's owner |
| Destinations: Matrix, Slack, Fediverse | [ADR-0004](.docs/decisions/0004-each-destination-gets-its-own-dialect-limits-and-features.md) | the Fediverse through the Mastodon client API, Matrix through the client-server API, and Slack by bot token or incoming webhook, with subscribers mentioned, against reference fixtures; the ActivityPub actor's signed deliveries of each Note to its followers' shared inboxes, each follow kept and answered with a signed `Accept` and each undo forgetting its follower, and its key loaded by every run and by `herald doctor`; `herald server` serving its inbox, its Notes and the actor and WebFinger documents ([ADR-0014](.docs/decisions/0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md)) |
| Commands: `subscribe`, `unsubscribe`, `latest` and `changelog`, and the subscribe button | [ADR-0004](.docs/decisions/0004-each-destination-gets-its-own-dialect-limits-and-features.md), [ADR-0016](.docs/decisions/0016-commands-are-carried-every-way-a-destination-offers-and-answered-from-what-herald-keeps.md) | on Discord over HTTP interactions and the gateway, on Slack over HTTP and Socket Mode, and on Matrix through an application service and a sync; each project's latest announcement kept by every store but markers |
| Stores: SQLite, Postgres, D1, DynamoDB, Deno KV, markers, git branch, Actions cache, repository variables | [ADR-0005](.docs/decisions/0005-an-announcement-is-sent-once-and-finished-if-interrupted.md) | SQLite, Postgres, the markers, the git branch, the Actions cache and repository variables; D1, DynamoDB and Deno KV, no |
| The CLI, `herald doctor` and the Action | [ADR-0002](.docs/decisions/0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md), [ADR-0009](.docs/decisions/0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md) | `announce`, `poll`, `housekeep`, `server`, `health`, `doctor`, `store`, `preview`, `init`, `token new`, `discord commands`, `discord roles`, `slack manifest` and `matrix registration`, and the Action; no release carries them yet |
| Serverless drivers and the container | [ADR-0002](.docs/decisions/0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md), [ADR-0009](.docs/decisions/0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md), [ADR-0014](.docs/decisions/0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md) | `herald server`, the container's command, `herald lambda`, Azure Functions' custom handler, the wasm module on Cloudflare Workers, Deno, Node, Bun, Vercel and Netlify, the `wasi:http` component on wasmtime, Spin and Fastly, and the image with its Compose, Kubernetes and Helm templates, each run in CI ([deploy](deploy/README.md)) |
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
