# Pull requests

A forge source whose `signals` include `"pull-request"` announces the
repository's pull requests, and GitLab's merge requests, as `pull-request`
events. An owner source announces its members', and a member override's
`signals` chooses them per repository.

```toml
[sources.cli]
kind = "github"
repository = "nightworksio/herald"
signals = ["release", "pull-request"]
pulls = { actions = ["opened", "merged"], authors = ["dependabot[bot]", "renovate[bot]"] }
```

## The keys

| Key | What it chooses | Where absent |
|-----|-----------------|--------------|
| `pulls.actions` | what a pull request did: `opened`, `ready_for_review`, `reopened`, `merged`, `closed` (closed unmerged) | `["opened", "ready_for_review", "merged"]` |
| `pulls.drafts` | whether a draft's opening is announced | `false`: a draft is announced once ready for review |
| `pulls.labels` | the labels, as globs, any one of which it carries | every pull request |
| `pulls.authors` | the authors, as globs | every author |

## The event

Its title says what it did, as in `Pull request #4 merged: Bump left-pad`
or `Merge request !3 opened: Add feature`, and its `pull` link opens it.

| Attribute | Holds |
|-----------|-------|
| `signal` | `pull-request` |
| `action` | what it did |
| `number` | its number in the repository |
| `author` | who opened it, where the forge names them |
| `bot` | whether its author is a bot or an app |
| `base`, `head` | the branches it merges into and from |
| `commit` | its head commit's sha, where the forge names it |
| `draft` | whether it is a draft |
| `merged_by` | who merged it |
| `labels` | its labels, joined by a comma and a space |

Its identity is `<host>/<repository id>/pull/<number>/opened` for its
opening, which a webhook and a poll name alike, and
`.../<action>/<unix seconds>` for what it did after.

## Areas

A route's `match.areas` takes only the pull request events whose pull
request changes a path in one of the areas its sources name (ADR-0071,
ADR-0072), and `match.not.areas` none of them. An event's
`origin.areas` comes from the paths kept for its pull request where the
kept head is the event's `commit`, so a view must read the source kept;
`area_paths = "delivery"` on the route reads them from the forge as the
event is delivered instead, one request an event. Where the paths are
not all known, the event falls in no area: `match.areas` does not take
it and `not.areas` refuses it. Areas travel with the event, so a private
repository's reach only the routes said private.

```toml
[[routes]]
id = "web-team"
sources = ["monorepo"]
destination = "discord"
channel = "1566200000000000100"
area_paths = "delivery"
match = { areas = ["web"] }
```

## How pull requests reach herald

| Path | GitHub | GitLab | Forgejo |
|------|--------|--------|---------|
| Webhook | `pull_request` | Merge Request Hook (`merge_requests_events`) | `pull_request` |
| Poll | `/pulls`, most recently updated first | `/merge_requests`, most recently updated first | `/pulls`, most recently updated first |

A poll reads the listing under its validators and announces the openings,
merges and closes among the newest it lists; a pull request readied for
review or reopened comes only by webhook, and doctor says so where the
source names none. GitLab's webhook names only who acted, so a merge
request's author is known from its opening alone and `pulls.authors` takes
no other GitLab webhook event; a GitLab bot is a project or group access
token's (`project_<id>_bot_…`) or a name ending in `-bot` or `[bot]`. An
owner source announcing pull requests polls every member each round.
