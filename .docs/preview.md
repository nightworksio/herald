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
| `--event <file> --from github\|gitlab\|forgejo` | the release a forge's release event publishes, on each route that takes it, in place of a sample |

| `--format text\|wire\|json` | what is printed of each route's message: `text` (the default) as above, `wire` the requests its first post sends, `json` both |

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
  peer says why it shows no request: an actor's deliveries go to followers
  preview does not know, a feed is served rather than sent, and a push is
  signed by the instance's key, which preview never reads.

`--format json` prints one JSON array, on one line, with an object per
route and what it renders against: `route`, `destination`, `against`
(`{"kind", "sample"}`, `{"report"}`, `{"group"}`, or `null` for an event
file), then either `error` (why its templates cannot render) or `title`,
`thread`, `parts`, `steps` (each `{"step", "requests"}`, a request as
`method`, `url`, `headers` as `[name, value]` pairs and `body`, the JSON it
holds, its text, or `null`) and `unsent` (why it shows no request, or
`null`).

A flag that names no route, kind, sample or report of the config says
which ones it has, never what was given. Where no route takes what the
flags ask for, it says so and exits 2; where a template cannot render, it
says why and exits 1.

`herald announce --dry-run` prints the requests a first announcement of a
run's own event sends, secrets redacted; it reads the forge and the
secrets the run uses.
