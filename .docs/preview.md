# Preview: what herald would send, before it sends anything

`herald preview` renders each route's templates as its destination would
show them, in the destination's dialect and within its limits
([ADR-0095](decisions/0095-preview-renders-every-kind-as-its-destination-sends-it-reading-nothing-and-a-gallery-is-generated-from-it.md)).
It resolves no secret and opens no connection: the config is all it reads,
and every marker is a placeholder.

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

A flag that names no route, kind, sample or report of the config says
which ones it has, never what was given. Where no route takes what the
flags ask for, it says so and exits 2; where a template cannot render, it
says why and exits 1.

`herald announce --dry-run` prints the requests a first announcement of a
run's own event sends, secrets redacted; it reads the forge and the
secrets the run uses.
