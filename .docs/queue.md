# The queue: views and reports

A view names which open pull requests herald shows, and whose turn each
is. A report announces a view once a window through the routes naming it,
and `herald queue` prints one now. Each reads a source naming one
repository on GitHub, GitLab or Forgejo, live, as the report or command
runs.

```toml
[sources.herald]
kind = "github"
repository = "nightworksio/herald"
token = { secret = "HERALD_GITHUB_TOKEN" }

[views.review]
states = ["waiting-on-review", "waiting-on-merge"]
group_by = "repository"

[reports.morning]
view = "review"
at = "09:00"
days = ["mon", "tue", "wed", "thu", "fri"]
zone = "Europe/Amsterdam"

[[routes]]
id = "queue"
sources = ["morning"]
destination = "discord"
channel = "1566200000000000100"
template = "herald:queue"
private = false
```

## Views

The view `queue` is built in: every source naming one repository, as
`View`'s defaults below set it. A `[views.queue]` table replaces it.

| Key | What it chooses | Where absent |
|-----|-----------------|--------------|
| `sources` | the sources it reads, each naming one repository on a forge | every such source |
| `states` | the states it shows | every state |
| `bots` | `include`, `exclude`, `only`, or `batch`: a bot's pull requests gathered into batches of the same bump across repositories | `batch` |
| `repositories`, `authors`, `labels` | globs, ignoring case, of the full path, the author's login, and any one label | every one |
| `stale` | how long one waits with nobody acting before it is stale | `"14d"` |
| `stale_only` | whether it shows only the stale | `false` |
| `group_by` | `turn`, `repository`, `author` or `nothing` | `turn` |
| `deciding` | `herald`, `forge`, or `{ labels = [{ label = "…", state = "…" }] }` | `herald` |
| `rules.*` | each of herald's rules, switched off by `false` | each on |
| `bumps` | its own bump patterns, read before Dependabot's and Renovate's: `{package}`, `{to}` and `*`, ignoring case | none |
| `most_open` | the most open pull requests it lists of each repository | `500` |
| `most_pulls` | the most it reads in full where each takes requests of its own | `100` |

### Whose turn it is

With `deciding = "herald"` the first rule that holds tells it:

| Rule | State | Reason |
|------|-------|--------|
| `draft` | `draft` | `draft` |
| (read past `most_pulls`) | `unknown` | `unread` |
| `queued` | `queued` | `queued` |
| `conflict` | `waiting-on-author` | `conflict` |
| `changes` | `waiting-on-author` | `changes-requested` |
| `failed` | `waiting-on-author` | `check-failed: <names>` |
| `pending` | `waiting-on-checks` | `checks-pending` |
| `approval` | `unknown` where a review cannot be told, else `waiting-on-review` | `review-unknown`, `review-requested` or `unapproved` |
| (none) | `waiting-on-merge` | `ready` |

`deciding = "forge"` takes the forge's own status where it gives one:
GitHub's merge state, GitLab's `detailed_merge_status`. With labels, the
first label a pull request carries, ignoring case, tells its state; with
none it is `unknown`.

A row is stale once nobody has acted on it for `stale`, from its last
update.

### What a view reads

| Forge | With a token | Without |
|-------|--------------|---------|
| GitHub | GraphQL: a page of 100 holds reviews, checks and mergeability | REST: the listing, then each pull request's reviews and its head's check runs |
| GitLab | the merge request listing | the same |
| Forgejo | REST: the listing, then each pull request's reviews and its head's combined status | the same |

Where each pull request takes requests of its own, those past
`most_pulls` read as `unknown`. GitHub's REST listing names no
mergeability, so without a token no bot batch counts green. On REST, only
a reviewer GitHub names an owner, member or collaborator, or Forgejo marks
official, decides a review. `herald doctor` names each report's path and
the most requests it takes a round.

## Reports

| Key | What it sets | Where absent |
|-----|--------------|--------------|
| `view` | the view it shows | `queue` |
| `every` | how long each window lasts within a day, `"1m"` to `"1d"` | `"1d"` |
| `at` | the wall-clock time a day's first window begins, `"HH:MM"` | `"09:00"` |
| `days` | the days windows begin on: `mon` to `sun` | every day |
| `zone` | the IANA time zone `at` and `days` are read in | `"UTC"` |

A window begins at `at`, then each `every` after it until the day ends. A
report's name is a source a route names: no source or app may share it.
Each poll announces every report whose window began, after its sources;
`POST /herald/round/report` announces the reports alone, for a host that
times them apart. A report of a window is announced once: its identity is
`report/<name>/<window start>`, and herald remembers the window once no
route stopped.

The `report` event carries `view`, `pulls` and `stale`; its project is the
report's name. Apps and other heralds' feeds give no `report` event.

### Private repositories

A repository whose visibility is not known counts as private. Where a
view reads a private or internal repository and `[doctor]
private_to_public` refuses, routes declared `private = true` are shown
every row, and the others only the public repositories' rows, or nothing
where the view read none. With `"warn"` every route is shown every row
and doctor names each route that is not private; with `"allow"` it says
nothing.

## Templates

A route's template reads the board as `view`:

| Key | Holds |
|-----|-------|
| `view.name`, `view.total`, `view.stale` | the view's name, how many it shows, how many are stale |
| `view.counts` | each state with its `count`, as `{ state, count }` |
| `view.groups` | each group's `name` and `rows` |
| `view.batches` | each batch's `package`, `to`, `green`, `failing` and `rows` |
| a row | `project`, `repository`, `number`, `title`, `url`, `author`, `bot`, `draft`, `labels`, `opened`, `updated`, `state`, `reason`, `waited` (as `3d`, `5h` or `12m`), `waited_seconds`, `stale` |

herald ships `herald:queue`; a `[templates."herald:queue"]` table replaces
it.

## `herald queue`

```sh
herald queue                 # the built-in view, as a table
herald queue review --format json
```

The JSON holds the keys a template reads as `view`.

## Time zones

The time zone database is bundled into every build, which adds about
207 KB (49 KB compressed). A build without `herald-core`'s default `tzdb`
feature reads only `UTC`.
