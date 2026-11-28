# Preview: what herald would send, before it sends anything

`herald preview` renders each route's templates as its destination would
show them, in the destination's dialect and within its limits
([ADR-0095](decisions/0095-preview-renders-every-kind-as-its-destination-sends-it-reading-nothing-and-a-gallery-is-generated-from-it.md)).
It resolves no secret and opens no connection in any format: the config is
all it reads, and every marker is a placeholder.

```console
$ herald preview --sample plain
releases to discord, sample release (plain):
  title: 📦 sample-project v1.2.0
  part 1:
    ## Added

    - a sample change
```

Without a flag, each route renders every sample of each kind it takes:
the kinds its `kinds` names, or else every kind its sources announce. A
route that takes a report renders it showing a sample view, as the report
shows its view.

| Flag | What it renders |
|------|-----------------|
| `--route <id>` | only that route |
| `--kind <kind>` | only that kind's samples |
| `--sample <name>` | only that sample of each kind: `plain`, `pre-release` (a kind that marks pre-releases), `long` (a body past every destination's limit) or `no-body` (a kind that names a signal) |
| `--report <name>` | only that report, showing a sample view |
| `--group <key>` | each route that groups, its samples as one group under the key ([one post for a release train](../README.md#one-post-for-a-release-train)) |
| `--event <file> --from github\|gitlab\|forgejo` | a forge's event file (`-` for standard input), on each route that takes it, in place of a sample ([below](#an-event-file)) |
| `--format text\|wire\|json` | what is printed of each route's message: `text` (the default) as above, `wire` the requests its first post sends, `json` both |

## An event file

`--event` reads a payload as the forge's webhook delivers it. Each source
of the config reads it as its webhook reads a delivery, and each route
renders the event of a source it takes. `--event-name` gives the event's
name as the forge's event header (`X-GitHub-Event`, `X-Gitlab-Event`,
`X-Forgejo-Event`) carries it; without it, the forge's release event
(`release`, or GitLab's `Release Hook`).

```console
$ herald preview --event pull.json --event-name pull_request
pulls to discord:
  title: Pull request #4 opened: Bump
  part 1:
    …
```

- An event of a private or internal repository prints each route it
  reaches as `<route> to <destination>: (private)`, in every format,
  unless `--private` or `[cli] private` says the output is private
  ([ADR-0080](decisions/0080-a-poll-announces-a-milestone-overdue-by-its-own-clock-and-poll-and-announce-name-private-events-where-said-private.md)).
- Where no source maps the file to an event it announces, preview says
  what each source made of it and exits 2: a reason of its own (a draft
  release, a kind the source does not announce), a tag or package a poll
  reads in full, or a payload it cannot read as the event named. It names
  neither the path, the event name given nor anything the file holds; a
  file that is not JSON is named by the line and column alone.

## The requests a first post sends

`--format wire` prints, for each route, every step its delivery asks of
its destination (the post, its crosspost, thread, overflow parts and pin)
and each request the step sends, as the destination's own request builder
writes it: method, URL, headers and body, nothing sent.

```console
$ herald preview --sample plain --format wire
releases to discord, sample release (plain):
  post:
    POST https://discord.com/api/v10/channels/1566200000000000100/messages
    User-Agent: DiscordBot (https://github.com/nightworksio/herald, 0.0.0)
    Authorization: Bot «secret:HERALD_DISCORD_TOKEN»
    Content-Type: application/json

    {"allowed_mentions":{"parse":[],"roles":[]},…}
```

- Every secret is a placeholder that holds no value and shows as
  `«secret:NAME»`: a bot's token in its header, a webhook's whole URL
  (`POST «secret:HERALD_SLACK_WEBHOOK»`). A sealed body shows its length
  alone.
- An id a platform gives an earlier step (the post's, its thread's) is
  `0`, as `herald announce --dry-run` reads every answer.
- It is the route's first announcement: a pin kept in the store
  (`pin = "store"`) unpins nothing, while one found among the
  destination's pins (`pin = "markers"`) shows the requests that look.
- A route to one of herald's own ActivityPub actors, feeds or pushes to a
  peer shows the document herald publishes in place of a request, marked
  `served, not sent`: an actor's Note as its outbox serves it and each
  follower's inbox receives it, a feed's envelope, or the envelope a push
  carries to the peer. Its proof and the signature of the requests that
  carry it are never shown, only the key that makes them, by name
  (`«secret:NAME»`), or the run's ID token a peer proves. A feed takes no
  event that is not public, and a push one only where the peer takes
  private events; each says so in place of a document.

`--format json` prints one JSON array, on one line, with an object per
route and what it renders against: `route`, `destination`, `against`
(`{"kind", "sample"}`, `{"report"}`, `{"group"}`, or `null` for an event
file), then either `error` (why its templates cannot render) or `title`,
`thread`, `parts`, `steps` (each `{"step", "requests"}`, a request as
`method`, `url`, `headers` as `[name, value]` pairs and `body`, the JSON it
holds, its text, or `null`), `unsent` (why it shows no request, or `null`)
and `served` (the document herald publishes in place of a request: `by` as
`actor`, `feed` or `peer`, `name` the actor's or peer's name, `proof` the
key's name or the ID token, and `document` as JSON; or `null`).

A flag that names no route, kind, sample or report of the config says
which ones it has, never what was given. Where no route takes what the
flags ask for, it says so and exits 2; where a template cannot render, it
says why and exits 1.

`herald announce --dry-run` prints the requests a first announcement of a
run's own event sends, secrets redacted; it reads the forge and the
secrets the run uses.
