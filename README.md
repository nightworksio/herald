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

- forge releases and CI runs from GitHub, GitLab and Forgejo or Gitea,
  from one repository or every repository an owner holds;
- package versions from Packagist, crates.io, npm and Docker Hub, and
  image tags from ghcr and any OCI registry;
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

`herald doctor` names every feature a config asks for that the layer it runs in
cannot serve, before anything posts, with the first layer that serves it.
`--layer` names the host to check for: `actions`, `server`, `lambda`, `azure`,
`worker`, `deno`, `node`, `vercel`, `netlify`, `wasi`, `spin` or `fastly`. It
says what asks each host for rounds, and where `[schedule]` disagrees with the
cron its deploy files ship. `--format json` prints the same findings as one
JSON document, which [`herald-doctor.schema.json`](herald-doctor.schema.json)
describes. Both show the instance among other heralds
([federation](.docs/federation.md)). `--online` also reads every secret and
asks each destination, with nothing posted, whether it takes its credential and
grants what each route needs: a Discord bot's permissions for the route's
features, a Slack bot token's scopes, a Matrix bot's power level in its room, a
Mastodon server's own character limit against the route's `limit`, and a Slack
incoming webhook by an empty post Slack refuses before anything reaches the
channel. `--online --post` also posts one test message to each route's
destination, a plain post with no crosspost, thread, pin or mention, kept in
memory so no real announcement is held back by it. A destination's interactions
word it with `test_title` and `test_body`, which `{{ route }}` names the route in.

**Never twice, never half-posted.** Each announcement has a deterministic
key, and every step that lands is recorded: the post, the crosspost, the
thread, each overflow chunk and the pin. A rerun finds the key and sends
nothing. An interrupted run is finished by the next one, which sends only
what is missing.

## Quick start

Every key `herald.toml` takes is listed, with its type and what it sets, in the [settings reference](.docs/settings.md), generated from the schema.

The snippets below show the configuration that [ADR-0006](.docs/decisions/0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md) and [ADR-0009](.docs/decisions/0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md) fix. No release of herald exists yet, so the Action has no binary to download.

`herald.toml`, herald's own, announcing its releases and pre-releases into the NightWorksIO Discord's `package-releases` announcement channel:

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
thread = { name = "{{ event.project }} {{ event.attributes.version }}", archive_after = "14d" }
pin = "markers"

[templates.package-release]
title = "📦 {{ event.project }} {{ event.attributes.version }}{% if event.attributes.prerelease %} · pre-release{% endif %}"
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
so the `concurrency` group keeps to one herald run at a time.
`[markers] key` names the secret every route's markers are keyed with; where
it is absent, a route's key is derived from its destination's credential, so
a new credential gives the route new markers. `herald init` prints a new
`HERALD_MARKER_SECRET`, once. Each destination reads back up to
`read_back` pages of its own posts, 5 by default and at most 20, and stops at
the page holding the post it looks for. A rerun that finds a post an earlier
run attempted but cannot read the channel back stops rather than post it
twice. Discord, Slack and Matrix look up at most `pin_lookups` pins, 25 by
default and at most 100, as they find a route's earlier pins, and say so
where more are pinned. A route's `thread` takes `archive_after`, how long
after the announcement housekeeping archives the thread, and `lock_after`,
how long after the announcement housekeeping locks and archives it.

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

The destination's `deliveries` (8) sets how many of an announcement's
followers' inboxes it delivers to at once, whichever command announces.

Store the file's contents as `HERALD_ACTIVITYPUB_KEY`. Every run loads the
key before it sends anything. `herald doctor` loads it too, wherever it is
set.

People find and follow the actor through `herald server`.

A route can announce each project as an actor of its own, named by a
template that reads `event.project`:

```toml
[[routes]]
id = "per-repository"
sources = ["lemonfiber"]
destination = "fediverse"
actor = "{{ event.project | lower | replace('.', '-') }}"
template = "package-release"
```

The route then serves an actor for each project its sources and apps name
and each repository its owner sources take, so `@plugin-sonarr@herald.example.org`
follows one repository. A project whose name is no actor's name (at most
30 lowercase letters, digits, `_` or a `-` within) is not announced on the
route; `herald doctor` names it. Each name is one actor's: a project
whose name a route's own actor holds, or that another project names too,
is not announced as it, and `herald doctor` names both. A repository that leaves its owner's
selection keeps its actor while it has followers, taking no new follows,
until its last follower leaves or `keep_members` passes; herald then
answers that the actor is gone.

### herald among other instances

`[federation]` names the domain herald serves on and its Ed25519 keys; a
`herald` destination publishes a signed feed of herald's own, and
`[federation.pages]` writes it as a signed log a static host serves;
`[peers.*]` pins the other heralds this one trusts, by the keys their
operators confirm with `herald peer add`; a `herald` source pulls a
peer's feed through those keys, and takes what the peers it grants
`push` send to its inbox, each request kept in the peer's audit of the
day for `herald peer log`; a `herald-push` destination pushes each event
to a peer's inbox. [.docs/federation.md](.docs/federation.md) covers each.

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
- `busy_retry` ("30s") is how long a request whose work found no place is asked to wait, `read` ("30s") and `held` ("90s") how long a connection may take to send its headers and how long it is held at most, and `accept_pause` ("100ms") how long the server waits after a connection could not be accepted.
- `[server.sessions]` sets how a session a destination holds connects again: `first_wait` ("1s"), doubled up to `longest_wait` ("1m"), each wait between half of it and all of it unless `jitter = false`. The store keeps where a Matrix sync stands, so a sync taken up again answers the commands given while none was held, unless they are older than `stale` ("10m"). A session whose credential is refused stops.
- `base`, such as `"/herald"`, serves every endpoint under that path, for a reverse proxy that serves herald beside other things; the ActivityPub actors' ids, and the addresses `slack manifest` and `matrix registration` print, are under it too.
- `[server]` also sets `jobs` (how many pieces of work it does at once: announcements, activities and commands), `queue` (how many pieces of work may wait for a place, as many as `jobs` where absent), `connections`, `body`, `fetches`, `completions` (how many autocompletions a session answers at once) and `silence` (how long a session hears nothing before it checks its connection), and `[server.keys]` sets where inbound activities' keys are kept (`cache = "memory"` or `"store"`), `refresh_after`, `reread_after`, `most` and `document` (the most of a key's document read) ([ADR-0014](.docs/decisions/0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md)).

`herald doctor --layer server` names what the config asks for that the
server cannot serve.

`[http]` sets how herald's own requests reach the services the config names:
`connect` ("10s"), `total` ("30s", or `HERALD_TIMEOUT_MS` in its place),
`proxy` (`"environment"`, the default, which honours `HTTPS_PROXY` and
`NO_PROXY` where the host does; `"none"`; or a proxy's URL), `ca_file`, a
PEM file of certificate authorities trusted beside the Mozilla roots, such as
a GitHub Enterprise Server's private CA, beside `herald.toml` or in a directory
`[files] allowed` names, as every file the config names is, and `body` ("32MiB", at most
"128MiB"), the most of a response herald reads. A proxy URL and `ca_file` are for the
native binary, the image and Lambda; other hosts refuse them, naming why. None
of it applies to the hosts herald reaches on others' behalf, which keep
ADR-0007's 5 and 10 seconds at most, read no more than "32MiB", go through no
proxy and trust the Mozilla roots alone.

`[server.inbound]` sets what a signed request may carry and still be
checked: `replay_window` ("5m"), `signatures` (4) and `covered` (16, the
headers one ActivityPub signature may name). `[render]` sets how far a
template may go: `fuel` (100000), `recursion` (16) and `output` ("256KiB").
Each of these, `[server] body`, `[server.keys] document` and `[http] body` may be at most four times its default,
and `herald doctor` names any set above it.

`[announce]` sets `lease` ("20m"), how long a run holds an announcement it
claimed before another may finish it, and `latest_attempts` (3), how often a
run writes what a project last announced again when another write lands
first.

`[run]` sets how many routes an event is announced to at once, `routes` (4),
and how many sources a poll reads at once, `sources` (4). 1 goes one after
another in the file's order. On the `git-branch`, `forgejo-branch`,
`repo-variables`, `actions-cache` and `markers` stores, which write one record
at a time, routes and sources always go one at a time.

Each source sets how a poll reads it: `backfill`, how many of the newest
releases, versions or entries the first poll announces, none where absent;
`page` (30), how many of the newest releases, versions or entries one poll
reads; `remembered` (500, at least `page`), how many its cursor remembers;
and `parse` ("4MiB", "16MiB" on Packagist and "32MiB" on npm, and at most four
times that), the most of an answer it parses.

Housekeeping also forgets what the store keeps past `[store]`'s
`keep_deliveries` (each webhook delivery's id, "7d", and at least "3d"),
`keep_ledger` (each ledger entry and its deliveries to followers' inboxes,
"90d" after a run last claimed it) and `keep_notes` (each Note herald's
actors published, "forever"). `keep_ledger` and `keep_notes` also take
`"forever"`.

Every store that keeps records also sets `attempts` (3, at most 12), how
many times it tries a write that lost a race to an unrelated write. The
`repo-variables` store sets how its lock is taken as abandoned:
`lock_lease` ("10m", at most "40m"), how long a lock holds by GitHub's
clock, and `lock_patience` (10, at most 40), how many times a lock with no
time to judge it by is found held and unchanged. The `fastly-kv` store sets
`tombstone` ("1h"), how long a deleted record's mark is kept before Fastly
expires it. `herald doctor` names each of `attempts`, `lock_lease` and
`lock_patience` set above its default.

### Every repository an owner holds

A forge source can name an `owner`, an organization, GitLab group or user,
in place of a `repository`. Each round lists the owner's repositories and
polls each one it takes on its own, with its own cursor:

```toml
[sources.plugins]
kind = "github"
owner = "lemonfiber"
include = ["plugin-*"]
exclude = ["plugin-template"]
token = { secret = "GITHUB_TOKEN" }
```

`include` and `exclude` are case-insensitive name globs (`*`, `?`, `[...]`),
and `exclude` wins. `topics` and `language` take any of their values, and
`properties` (GitHub's custom properties) needs every named property to hold
one of its values. Archived repositories, forks and templates are left out
unless `archived`, `fork` or `template` is true, and only public
repositories are taken unless `visibility` names `private` or `internal`. A
GitLab group's subgroups are included unless `subgroups = false`. A user's
listing on GitHub holds the repositories anyone may see. The listing is
asked at the same `api` as everything else, so GitHub Enterprise Server,
GHE.com and self-managed GitLab work alike.

A repository found after the first listing announces only what it publishes
after it was found; the members of the first listing announce their newest
`backfill`. A repository that leaves the selection stops being polled at
once, and its cursor is kept for `keep_members` ("30d") in case it comes
back. Each page of the listing is asked with its `ETag`, so an unchanged
owner costs one request per page and one for the page after the last. A
listing that fails keeps the members the last one found. A listing over
`most_listed` (10000 repositories before the filters) or a selection over
`most_members` (1000, at most 100000) refuses the source for the round and
names the setting.

A route that takes private or internal repositories must say its place is
private with `private = true`; otherwise the config does not load, and an
event whose `event.origin` says not everyone may see its repository reaches
only `private` routes, from an owner's member and a single repository alike.
A repository whose visibility the forge does not name counts as private.
`[doctor] private_to_public = "warn"` loads such a config and doctor names
the route, and `"allow"` says nothing. Each member is checked against the
current filters every round, and a listing kept for other filters, another
owner or another `api` is read whole. `herald doctor --online` lists each owner and
names any two repositories, of one owner or of two, that would announce as
the same project.

A member file names the repositories an owner source takes, and the filters
narrow it. It is a TOML or JSON file beside the config, read once as the
config loads, or a file in a repository on the same forge, read through its
API with the source's token each round and asked with its `ETag`. `tables`
names the arrays that list repositories and `key` the field naming each one
("name" where absent); an entry matches a repository's name or full path,
ignoring case. `herald doctor --online` names the entries the owner does
not hold.

```toml
[sources.org]
kind = "github"
owner = "lemonfiber"
members = { repository = "lemonfiber/spec", path = "30-repos/repos.toml", ref = "main" }
tables = ["repo", "ungoverned"]
project = "{{ repository.owner }}/{{ repository.name }}"
webhook = { secret = "HERALD_WEBHOOK" }
```

`project` names the project a member's events are announced as, from
`repository.name`, `path`, `owner`, `id`, `topics`, `properties` and
`visibility`; a template that reads anything else does not load. An owner
source takes webhooks from each repository and from the organization. A
delivery is announced only where the repository is one the owner holds and
the filters take by what the payload names, and only where the owner's last
membership record holds it when the work runs, so poll the source as well. A delivery
reaches a public route only where both it and the record say the
repository is public.

A GitHub owner with a `token` polls its members through GraphQL, 50 to a
request (`graphql_batch`, at most 100): one request asks each member's
newest releases by tag, and a second asks in full only those holding one
not yet seen. `transport = "rest"` polls each member with its own request,
as an owner without a token does, and `transport = "graphql"` insists on
GraphQL. The endpoint sits beside `api` (`/api/graphql` on a GitHub
Enterprise Server, `/graphql` on GitHub and GHE.com); `graphql` names
another on the same host.
`transport = "events"` reads the owner's events once a round, with its
`ETag`, and polls only the members a published release names (or a new
tag, where a member takes tags), and those never polled; where the events do not reach back to the last read, every
member is polled.

A GitLab owner with a `token` polls its members through GitLab's GraphQL
API the same way, asking each batch's projects by id; `transport =
"graphql"` asks it without a token too, as public projects need none. The
endpoint sits beside `api` (`/api/graphql` beside `/api/v4`, under any
relative root). GitLab's GraphQL API lists no tags or packages, so an owner
whose members take tags or packages polls each over REST, and does not
load with `transport = "graphql"`. A Forgejo owner polls each member over
REST; `transport = "feed"` reads the organization's or user's activity
feed once a round instead and polls only the members it names publishing a
Release (or pushing a tag, where a member takes tags), as GitHub's events
do: `feed_page` activities to a page (50, at most 200) and `feed_pages`
pages back (3, at most 12).

`[sources.<owner>.repositories."<glob>"]` overrides `project`, `signals`,
`prerelease`, `routes` and `routes_only` for the repositories a glob names, each key settled by the narrowest glob setting
it: one without wildcards before one with, and otherwise the one naming
more characters. Two equally narrow globs that set one key and could name
one repository do not load. A repository's admin controls its
name, topics and properties, so no member announces as a project another
member, source or app announces as: such a member is withheld and
reported until the two differ.

`discovery = "push"` lists the owner only where the source holds no
listing, its filters changed, or a delivery said the owner's repositories
changed (ADR-0041); `"list"`, the default, lists it each round. GitHub's
`repository` and `installation_repositories` events and Forgejo's
`repository` event, from a repository the owner holds, have herald list
the owner again and poll it at once, under either setting. herald reads no
GitLab event of a project joining a group, so doctor names a GitLab source
discovering by push, one that names no webhook, and one on a layer with
no endpoint.

A GitHub source can read as a GitHub App in place of a `token`. herald
mints an installation token before each round, keeps it in memory, and
mints the next one five minutes before it expires:

```toml
[github_apps.herald]
client_id = "Iv23liABCDEF"
private_key = { secret = "HERALD_APP_KEY" }
webhook = { secret = "HERALD_APP_WEBHOOK" }

[sources.org]
kind = "github"
owner = "lemonfiber"
app = "herald"
```

`store_token = true` keeps each token in the store as well, so a later run
reads it in place of minting one. The token, good for an hour at most, is
then a credential at rest: herald encrypts it with AES-256-GCM under a key
derived from the App's private key, bound to its record and its expiry so
it opens nowhere else and never later, and a store that keeps no records
refuses the setting. A token the store does not keep is still read with:
the round notes it for each source, naming the store, and
`herald doctor --online` names a store that refuses it. `app_id` may stand in for `client_id`. The private key is read as GitHub
hands it out (PKCS#1) or as PKCS#8. `api` names GitHub Enterprise Server's
or GHE.com's REST API, and every source naming the App reads the same host.
The owner's installation is looked up as an organization's, then a
user's, and a repository's at the repository; `installation` fixes its id.
A source whose token could not be minted is not polled that round, and
says why. The App's webhooks arrive at `/hooks/app/<name>`, and each
delivery goes to every source reading as the App that takes it: an owner
source whose owner holds the repository, or the source of that repository.

GitLab sends a group's own webhooks only on Premium or Ultimate. On Free,
`member_hooks` gives each member a project webhook of its own, pointing at
`<[server] url>/hooks/<source>` and signed with the source's `webhook`:
`"setup"` registers them when you run `herald hooks <source>` (`--url`
names herald's address in place of `[server] url`), and `"round"` has each
poll round register one on a member that has none, which needs a token with
the `api` scope that may maintain each project. `"off"`, the default,
leaves the group to its own webhook or to polling. `herald doctor --online`
names each member with no webhook to herald, and under `"round"` a token
without the `api` scope.

### Where each event goes

Every forge and registry event carries `event.origin`: `forge` and `host`
(the source's `kind` and its host, so GitHub Enterprise Server, GHE.com and
self-managed GitLab name their own), `owner`, `repository` (the full path,
or the package's name), `id`, `topics`, `properties`, `visibility`, and
`source`, the source or app it came through. A registry's `owner` is the
package's scope, vendor or namespace, and its packages are public with no
topics or properties. Templates read it as `{{ event.origin.repository }}`;
a template reading it loads only on a route whose every source gives one.

A route's `match` narrows what its sources give; `sources` stays required.
Each list takes any of its values and every event where empty, and every
key must hold:

```toml
[[routes]]
id = "plugins"
sources = ["org", "registry"]
destination = "discord"
channel = "1566184281406177531"
template = "release"
match = { projects = ["plugin-*"], repositories = ["*/plugin-*"], owners = ["lemonfiber"], forges = ["github", "npm"], hosts = ["github.com"], topics = ["media"], properties = { team = ["core"] } }
```

`projects`, `repositories` and `owners` are case-insensitive globs.
`properties` needs every named property to hold one of its values (any
value where none is given), and `attributes` every named attribute of the
event's kind, compared as text ignoring case. `not`, written as a `match`
is, leaves out whatever it takes. An event with no origin, such as a
feed's, passes only a `match` that names projects and attributes alone.
A route whose `kinds` names a kind none of its sources announces is
refused, as is an attribute no kind it takes carries.

An owner's member can also name routes: `routes = ["plugins"]` under
`[sources.<owner>.repositories."<glob>"]` adds those routes for its events,
whatever sources they name, and `routes_only = true` makes them the only
ones. A route named so still narrows by its kinds, pre-releases, versions
and `match`. A route not declared private refuses such an override where
the owner takes private or internal repositories, as above. A route
announcing each project as an actor of its own serves only the projects
its `match` and its members' overrides take.

### Presets

A `[presets.<name>]` table adopts a preset herald ships, or a local one beside `herald.toml`:
routes, templates, views, reports and rules written as the config loads, overridden key by key ([`.docs/presets.md`](.docs/presets.md)).

### Releases, tags and pre-releases

A forge source announces each published Release by default. `signals`
chooses what it announces of each repository: `"release"`, `"tag"`, or
both:

```toml
[sources.train]
kind = "github"
owner = "lemonfiber"
signals = ["release", "tag"]
prerelease = "tag"
```

A tag and a Release of it share one identity, so a route announces the
version once, whichever comes first. Both are `release` events, and a
template tells them apart by `event.attributes.signal` (`"release"` or
`"tag"`). A tag's title is its name, its body its annotated message, and
its time the annotated tag's date, or else its commit's. Releases and tags
are each asked with their own `ETag`. On GitHub's REST API a new tag costs
up to two more requests to read its date and message; GitLab lists them in
full, and a GitHub owner polled through GraphQL reads them in its batch.
A webhook of a tag made (GitHub's and Forgejo's `create`, GitLab's tag
push) polls its source at once, so the tag is read in full and announced
or held like any tag a poll finds.
`herald announce --from <forge>` in a run pushing a tag, or for the tag
`--release` names or a GitLab tag pipeline is for, announces that tag where
its forge has no Release of it, and holds it where the source holds tags.

Where a repository's signals take both, `prefer = "release"` (the
default) holds a tag with no Release for `tag_window` ("1h", at most
"7d"), counted from when herald first sees the tag. A Release that arrives
in time is announced instead; otherwise the next poll or housekeeping round
after the window announces the tag. The held tag is kept in the store, so
the markers store, which keeps none, needs `prefer = "first"`, which
announces whichever comes first.

`prerelease` chooses how a pre-release is told: `"either"` (the forge's
flag, or a pre-release part in the version such as `v1.2.0-rc.1`; the
default), `"flag"` (the flag alone) or `"tag"` (the version alone). A tag
carries no flag, and neither does a GitLab release, so `"flag"` marks no
tag and does not load on GitLab.

### Packages

`signals = ["package"]`, beside or in place of the others, announces each
new version of the packages the forge's registry holds for the
repository, as a `package-version` event whose `signal` attribute is
`"package"` and whose `package` and `package_type` attributes name the
package and its type:

```toml
[sources.herald]
kind = "github"
repository = "nightworksio/herald"
token = { secret = "GITHUB_TOKEN" }
signals = ["release", "package"]
package_types = ["container", "npm"]
packages = ["herald*"]
```

`package_types` narrows the types (every type where absent; GitHub lists
npm, maven, rubygems, docker, nuget and container) and `packages` the
names, as globs. A container image announces each of its tags but those
`moving` names (`["latest"]` where absent), each under the registry's
identity `<registry>/<owner>/<image>/<tag>`, so a registry source of the
same image shares it and a route announces it once. `registry` names the
registry's host where it is not the default: ghcr.io beside GitHub,
`containers.<host>` beside a GitHub Enterprise Server or a GHE.com tenant,
each image's own on GitLab, and the instance's on Forgejo. Other types
take `<forge host>/<type>/<owner>/<package>/<version>`.

GitHub and Forgejo list an owner's packages, of which the source keeps
those linked to its repository; GitHub answers only with a token that may
read packages. An owner source lists them once a round for all its
members, and polls each member the listing names even where it follows
the owner's events. A package is seen as the stricter of its own visibility
(GitHub's package visibility, the project's on GitLab, the owner's on
Forgejo) and its repository's, so a private package of a public
repository, and a public package of a private one, reach only `private`
routes under `[doctor] private_to_public = "refuse"`, the default.
`package_visibility = "package"` lets the package's own visibility
decide. GitLab lists a project's packages, and its container
images' tags. A listing reads at most `most_packages` entries (1000, at
most 100000) before it refuses the round, each package's versions are
asked with their own `ETag`, and a round keeps the newest `page` versions
of a repository's packages. A new source announces its newest `backfill`
versions, as it does releases; a source that already polled before it
took packages remembers the versions it first finds without announcing
them. GitLab's image tags carry no time in their listing, so a new source
remembers the tags it first finds without announcing any. GitHub's `registry_package` and `package` webhooks and Forgejo's `package` webhook poll the source at once.

### CI runs, alerts, pull requests, issues and milestones

A forge source's `signals` also take `run`, `alert`, `pull-request`,
`issue` and `milestone`, each announced by webhook and poll with one vocabulary on every
forge, narrowed per source and matched by a route's `match.attributes`:

- `run`: CI runs, from GitHub Actions, GitLab pipelines, Forgejo Actions,
  and GitHub's check suites and statuses. [`.docs/runs.md`](.docs/runs.md)
- `alert`: security alerts, from GitHub's code scanning, Dependabot and
  secret scanning, and GitLab Ultimate's vulnerabilities. It is a private
  kind, so a route taking it says `private = true` or, on purpose,
  `private = false`. [`.docs/alerts.md`](.docs/alerts.md)
- `pull-request`: pull requests and merge requests opened, readied for
  review and merged, with a `bot` attribute. [`.docs/pulls.md`](.docs/pulls.md)
- `issue`: issues opened, closed and reopened. [`.docs/issues.md`](.docs/issues.md)
- `milestone`: milestones closed, opened, reopened, deleted and overdue, overdue found only by a poll. [`.docs/milestones.md`](.docs/milestones.md)

### The queue

A `[views.<name>]` table names which open pull requests of repositories
or whole owners herald shows and whose turn each is, bots' bumps batched;
it reads them live, or kept, from records its webhooks keep current.
A `[reports.<name>]` table announces a view once a window, at a wall-clock
time in its own time zone, through the routes naming the report, and the
shipped `herald:queue` template renders it. A `[boards.<name>]` table, or
a route with `living = true`, keeps one pinned post of a report, edited
each window. A `[rules.<name>]` table nudges each pull request that waited
past one of its `stages`, once a stage and spell of waiting. `herald queue`
prints a view now ([`.docs/queue.md`](.docs/queue.md)); a view's `decide` asks
models only what its rules cannot tell ([`.docs/deciders.md`](.docs/deciders.md));
a `[merge_queues.<name>]` table merges what a writer queues ([`.docs/merging.md`](.docs/merging.md)).

### Every package of a namespace

A registry source gives its registry's namespace key in place of
`package` to take every package of the namespace, each polled on its own
as an owner's repositories are:

```toml
[sources.acme]
kind = "npm"
scope = "@acme"
exclude = ["*-internal"]
```

npm takes `scope`, Packagist `vendor`, crates.io `owner` (`user:<login>` or
`team:github:<org>:<team>`) and Docker Hub `namespace`. `include` and
`exclude` choose packages by name, as globs. A listing reads at most
`most_listed` packages (10000) and keeps at most `most_members` (1000, at
most 100000), refusing the round over either, and a package that leaves
is kept `keep_members` ("30d") before its cursor is forgotten. Each
package's events take the identity and project a source of that package
gives.

npm lists `@scope/*` packages of the scope's organization;
`unscoped = true` takes its unscoped packages too. Where npm refuses the
listing, give a token that may read the organization, or name packages
one by one. Packagist lists a vendor through `/packages/list.json` at
`listing_api` (`https://packagist.org`), the one origin the source reaches
beside `api`, and never sends its token there; `listing = "composer"`
lists a Composer repository's `packages.json` at `api` instead, as
Private Packagist and Satis serve it. crates.io and Docker Hub list page
by page, `listed_page` (100, at most 100) to a page. A Packagist or
Docker Hub listing that names a package outside the vendor or namespace
fails, keeping the packages last listed, and `herald doctor --online`
names it. A listed name that is not a package's name, as
`package` takes one, fails the listing the same way.

Docker Hub names each repository's visibility. Elsewhere a package is
public where the source has no token and private where it has one, as a
token may list private packages; `visibility = "public"` declares a
namespace's packages public. Docker Hub tags take the identity
`docker.io/<namespace>/<repository>/<tag>`, the one a forge's container
package and any other source of the image share.

### An owner's container images

`kind = "ghcr"` takes a GitHub owner's container images, each polled on its
own and announced as the project of the repository it is linked to:

```toml
[sources.images]
kind = "ghcr"
owner = "nightworksio"
token = { secret = "GITHUB_TOKEN" }
```

GitHub lists packages only with a token that may read them. `api` names a
GitHub Enterprise Server or a GHE.com tenant, and `registry` the host the
images are named on (ghcr.io, or `containers.<host>` beside `api`, where
absent). `include` and `exclude` choose images by name, `visibility`
(`["public"]`) by who may see them, and `moving` (`["latest"]`) names the
tags never announced. A listing reads at most `most_packages` images
(1000, at most 100000), and an image that leaves is kept `keep_members`
("30d"). Each tag takes the identity `<registry>/<owner>/<image>/<tag>`, the
one a repository's `package` signal gives, so a route announces it once.

An image linked to a repository announces as that repository's project:
`project`, a template over its `repository.*`, or the repository's name. It
may share the project with the repository's own source; any other source
announcing as it withholds the image's events. An image linked to none
announces as its own name. An image is seen as the stricter of its own
visibility and its repository's; `package_visibility = "package"` lets the
image's own decide.

### An image on any registry

`kind = "oci"` takes an image's tags on any registry that serves the OCI
Distribution API, such as GitLab's, Quay, Harbor or a self-hosted one:

```toml
[sources.app]
kind = "oci"
registry = "https://registry.gitlab.com"
repository = "acme/app"
auth = "https://gitlab.com"
```

`catalog = true` instead takes every repository the registry's catalog
lists, each polled on its own, chosen by `include` and `exclude` globs on
its full path. The registry lists tags with no time, so the cursor
remembers every tag: the first poll records them all without announcing
any, and each later poll reads at most `page` new tags. A tag's time is its
image's `org.opencontainers.image.created` annotation, or else its config's
`created`, or else the time the tag was first seen; a time more than five
minutes past herald's own clock is not believed. A tag created before the
newest one announced is recorded without being announced. A listing reads at
most `most_tags` tags (10000, at most 100000). Where `most_tags` is not set
and the store keeps smaller records, such as git-branch or deno-kv, it is as
many as one record holds, and `herald doctor` names the number. `moving`
(`["latest"]`) names the tags never announced.

A token is fetched only from the registry's own origin or from `auth`
(`https://auth.docker.io` for Docker Hub's registry). `username` and `token`
read a private repository. A config blob is followed only to an origin
`blobs` names: by default Docker Hub's and GitHub's CDNs for their
registries. `herald doctor --online` names a token service or a blob origin
the source may not reach. Each tag takes the identity
`<registry_name>/<repository>/<tag>`, where `registry_name` is the registry's
host (`docker.io` for Docker Hub's), so a route taking the same image from
Docker Hub, ghcr or a repository's `package` signal announces it once.

### Edits to a late or edited release

A Release that comes after its tag was announced edits the tag's post to
the Release: the post and each chunk are edited in place, extra parts are
posted as chunks, parts no longer needed are deleted, and a Discord thread
opened from the post is renamed. A route's `late_release` chooses this:
`"edit"` (the default; a destination that cannot edit posts the Release
anew), `"post"` (always anew) or `"skip"` (leave the tag's post).
`edits = "release"` on a route also edits a Release's post when its name
or notes change on the forge; it is off by default:

```toml
[[routes]]
id = "train"
sources = ["train"]
destination = "discord"
channel = "1566184281406177531"
template = "release"
edits = "release"
```

The forge's own edit event (GitHub's `edited`, Forgejo's `updated`,
GitLab's `update`) is announced as an edit, and a poll watches the newest
`edits_depth` Releases (30, at most `page`) of a source a route edits
for, by a digest of their name and notes. Discord (bot or webhook),
Slack's bot, Matrix, Mastodon and ActivityPub edit; a Slack incoming
webhook does not, and the markers store records nothing of what a post
shows, so it edits nothing. ActivityPub sends the Note's `Update` to each
follower its `Create` reached; one Note carries every part, so it sends no
`Delete`.

### One post for a release train

A route's `group` gathers the events that share a key into one
announcement, such as every repository a train tags with one version:

```toml
[[routes]]
id = "train"
sources = ["org"]
destination = "discord"
channel = "1566184281406177531"
template = "train"
group = { key = "{{ event.attributes.version }}", window = "30m", lead = ["lemonfiber"] }

[templates.train]
title = "{% if group %}Release {{ group.key }}{% else %}{{ event.title }}{% endif %}"
body = "{% for member in events %}- {{ member.project }} {{ member.attributes.version }}\n{% endfor %}"
```

`mode = "hold"` (the default) holds the members for `window` ("30m", at
most "7d", from the first member) and announces them as one post at the
next poll or housekeeping round. `until = "lead"` posts as soon as the
lead comes. `mode = "edit"` posts the first member at once and edits the
post as the others come within the window, where the destination edits
a post. `lead` names, by project globs, whose event leads the post: the
first glob that matches a member wins, and the first member leads
otherwise. Templates read `events` (the members, lead first) and
`group.key`. Where an event is announced alone, `events` holds only that
event and `group` is none, so one template serves both.

A member that comes after its group was announced is posted alone
(`late = "post"`, the default), edited into the group's post
(`"edit"`), or left out (`"skip"`). A Release of a member the post shows
as a tag, or an edit of a member's Release, edits the group's post as
`late_release` and `edits` say. An event whose key renders empty or
longer than 256 bytes, or one past `most` (25, at most 100) or past what
the store keeps in a record, is announced alone. The markers store keeps
no groups, and a route to a herald feed or push, which carries each
event as an envelope of its own, takes no `group`. `herald preview --group v1.2.0` renders a route's group of
the sample releases.

### A deleted release

`withdraw = "delete"` on a route withdraws a Release's posts once the
forge deletes the Release (GitHub's and Forgejo's `deleted`, GitLab's
`delete`); `"keep"`, the default, leaves them. Each message herald posted
is deleted, the route's pin is let go, a thread opened from the post is
archived where the route archives threads, ActivityPub sends each follower
the Note reached its `Delete`, a herald feed lists a tombstone, and a
herald push pushes one to the peer. A
grouped member leaves the group's post, which is deleted once it shows
none. What the project last announced falls back to its stable release. A
Release published again later is announced anew. A destination that edits
nothing, such as a Slack incoming webhook, deletes nothing, and the
markers store cannot record a withdrawal. A withdrawal a run could not
finish is finished by the next round.

`herald withdraw releases github.com/212613049/v2.103.3` withdraws a
release from one route whatever its `withdraw` says, `--every-route` from
every route, and `--dry-run` prints what each would remove. The identity
is the forge's host, the repository's id and the tag.

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
tokens = ["sha256:50d858e0985ecc7f60418aaf0cc5ab587f42c2570a884095a9e8ccacd0f6545c"]
projects = ["herald"]
```

- `herald token new --app ci` prints a new token once, with the `tokens` line to keep. Add a second token before removing the first to rotate it. A CI run may present the ID token its CI issues instead: `oidc = [{ issuer = "https://token.actions.githubusercontent.com", claims = { repository_id = "123456" } }]` takes a token of that repository's runs meant for this herald, and `peers` lets those runs push into a herald source as the names it lists; [federation.md](.docs/federation.md#a-ci-runs-id-token-as-its-credential) says what each entry binds and how the issuer's keys are read.
- An app names at least one project, and at least one route names it among its sources, unless it only stands for `peers`; herald refuses a config where either is missing. A project is a case-insensitive glob: `projects = ["plugin-*"]` scopes the token to every plugin. Only a project named without a wildcard is the app's own, so a glob never withholds a member announcing as a matching project, and has no actor of its own.
- An event is herald's event JSON, as [`herald-event.schema.json`](herald-event.schema.json) describes it: `{"id": "7", "project": "herald", "title": "Deployed"}`, with `kind`, `body`, `links`, `attributes`, `occurred` and `origin` where there are any. An `origin` is checked as a forge's (`forge` a kind's name, `host` a host, `owner` and `repository` paths, an `id`), and herald names its `source` itself. Its kind is `message` where it names none.
- An app sends any kind the config knows: `release`, `package-version`, `feed-entry`, `message`, or one it declares. A declared kind names the attributes its events carry, and herald refuses an event whose attributes do not fit ([ADR-0018](.docs/decisions/0018-a-kind-is-data-and-an-event-carries-its-kind-s-attributes.md)):

  ```toml
  [kinds.deploy]
  description = "A deploy of a service."
  links = ["dashboard", { name = "status", label = "Status page" }]
  attributes = { region = { type = "text", required = true }, canary = { type = "flag" } }
  ```

  A template reads them as `{{ event.attributes.region }}`, and `herald doctor` names each attribute a template prints that a kind its route takes does not carry. A kind's links are its buttons, in order, each labelled by its `label`, by herald's wording for `release`, `changelog` and `entry`, or by its name ([ADR-0019](.docs/decisions/0019-a-kind-s-links-are-its-buttons-each-labelled-by-the-config-or-by-herald.md)).
- `herald announce --event deploy.json --app ci` announces it from a file, or from standard input with `--event -`.
- `herald server` takes it at `POST /api/projects/herald/events` with `Authorization: Bearer <token>`: 202 for a new event, 200 for one it has already ([ADR-0015](.docs/decisions/0015-an-app-is-a-source-routes-name-announcing-with-hashed-tokens-scoped-to-its-projects.md)).
- `GET /api/projects/herald/events/7`, with the same token, answers where its announcement stands on each of the app's routes: `pending`, `announcing`, `announced`, or `stopped` with the steps that remain. The project and the id are each one path segment, percent-encoded, so an id such as `deploy/7` is read back at `/api/projects/herald/events/deploy%2F7`.

### Commands in your channels

Members ask herald in the channel it announces in: `subscribe` and
`unsubscribe` to be mentioned when a project announces, `latest` for what it
announced last, and `changelog` for its notes. Each announcement can carry a
button that subscribes.

```toml
[destinations.discord]
kind = "discord"
token = { secret = "HERALD_DISCORD_TOKEN" }

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
  - Discord: `http`, posted to `/interactions/discord`, or `gateway`, held by `herald server` or a Cloudflare Worker's Durable Object.
  - Slack: `http`, posted to `/interactions/<destination>`, or `socket`, Socket Mode held by `herald server` or a Cloudflare Worker's Durable Object.
  - Matrix: `appservice`, pushed to `/matrix/<destination>`, or `sync`, as the bot, held by `herald server` or a Cloudflare Worker's Durable Object.
- A gateway connects only under `discord.gg` and Socket Mode only under `slack.com`; a resume or Socket Mode address elsewhere is refused. `domains = ["proxy.example"]` in the interactions adds domains, such as a proxy's.
- `herald discord commands` registers the commands. `herald discord roles --guild <id>` lists a server's roles, and `--create <name>` makes an opt-in role.
- `herald slack manifest --url https://<your host>` prints the Slack app's manifest. `herald matrix registration --url https://<your host>` prints the homeserver's registration with two new tokens, once, as the application service `herald-<destination>`, held to the homeserver's rate limits unless `rate_limited = false` in the interactions.
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
| The ports, fakes and the conformance suites every store, keyring, database, key-value store and host passes | [ADR-0001](.docs/decisions/0001-a-sans-io-core-behind-ports.md), [ADR-0005](.docs/decisions/0005-an-announcement-is-sent-once-and-finished-if-interrupted.md) | yes |
| `herald.toml` and its JSON Schema | [ADR-0006](.docs/decisions/0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md) | yes |
| Sources: forges, registries, feeds, apps | [ADR-0003](.docs/decisions/0003-every-source-becomes-one-neutral-event.md) | forge releases from GitHub, GitLab and Forgejo, by their event and by polling; package versions from Packagist, crates.io, npm and Docker Hub, one package or every package of a namespace ([ADR-0039](.docs/decisions/0039-a-registry-source-takes-every-package-of-a-namespace.md)), a GitHub owner's container images ([ADR-0040](.docs/decisions/0040-a-ghcr-source-announces-an-owner-s-images-as-their-repositories-projects.md)), and RSS and Atom feed entries, by polling; an app's events, through `herald announce --event` and `herald server` ([ADR-0015](.docs/decisions/0015-an-app-is-a-source-routes-name-announcing-with-hashed-tokens-scoped-to-its-projects.md)) |
| Destination: Discord | [ADR-0004](.docs/decisions/0004-each-destination-gets-its-own-dialect-limits-and-features.md), [ADR-0011](.docs/decisions/0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) | yes, against reference fixtures until recorded ones replace them |
| Inbound requests verified before they are parsed | [ADR-0007](.docs/decisions/0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md) | the signatures of GitHub, GitLab and Forgejo webhooks, Discord interactions and Slack requests, with the replay window, and the homeserver token of a Matrix transaction, each webhook delivery admitted once ([ADR-0013](.docs/decisions/0013-a-webhook-delivery-is-admitted-once-released-when-it-fails-and-kept-for-a-week.md)), and an ActivityPub activity's HTTP signature and digest against its actor's key, read from a public address on the key's own origin, with the activity's actor held to that key's owner |
| Destinations: Matrix, Slack, Fediverse | [ADR-0004](.docs/decisions/0004-each-destination-gets-its-own-dialect-limits-and-features.md), [ADR-0036](.docs/decisions/0036-a-route-announces-each-project-as-an-actor-of-its-own.md) | the Fediverse through the Mastodon client API, Matrix through the client-server API, and Slack by bot token or incoming webhook, with subscribers mentioned, against reference fixtures; the ActivityPub actor's signed deliveries of each Note to its followers' shared inboxes, each follow kept and answered with a signed `Accept` and each undo forgetting its follower, and its key loaded by every run and by `herald doctor`; `herald server` serving its inbox, its Notes and the actor and WebFinger documents ([ADR-0014](.docs/decisions/0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md)); a route announcing each project as an actor of its own, a departed repository's actor kept while followed and gone after |
| Commands: `subscribe`, `unsubscribe`, `latest` and `changelog`, and the subscribe button | [ADR-0004](.docs/decisions/0004-each-destination-gets-its-own-dialect-limits-and-features.md), [ADR-0016](.docs/decisions/0016-commands-are-carried-every-way-a-destination-offers-and-answered-from-what-herald-keeps.md) | on Discord over HTTP interactions and the gateway, on Slack over HTTP and Socket Mode, and on Matrix through an application service and a sync; each project's latest announcement kept by every store but markers |
| Stores: SQLite, Postgres, D1, a Durable Object, DynamoDB, Deno KV, Fastly KV, markers, git branch, Actions cache, repository variables | [ADR-0005](.docs/decisions/0005-an-announcement-is-sent-once-and-finished-if-interrupted.md), [ADR-0026](.docs/decisions/0026-a-store-s-write-attempts-lock-and-tombstone-are-settings.md) | yes |
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
