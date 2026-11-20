# The queue: views, reports, boards and rules

A view names which open pull requests herald shows, and whose turn each
is. A report announces a view once a window through the routes naming it,
a board keeps one post of it current, a rule nudges each pull request
that waited too long, and `herald queue` prints one now. Each reads a source naming one
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
| `mode` | `auto`, `live` or `kept` (below) | `auto` |
| `reconcile` | the least time between two listings of a source it reads kept | every round |

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

### An owner's repositories

A view may read a source naming an `owner` rather than one repository
(ADR-0062). It reads the owner's open pull requests by one query over
them, and keeps only those of a repository the owner's membership record
holds and has not left, so the source's selection applies. Each pull
request is shown as its repository's project and seen as the membership
record says, as the owner's releases are.

| Forge | The query | Then |
|-------|-----------|------|
| GitHub | `search/issues` for `is:pr is:open org:<owner>` (`user:` for a user), at most 1,000 | with a token, one GraphQL request for up to 100 pull requests across repositories; without, each pull request, its reviews and its head's check runs |
| GitLab | the group's `merge_requests`, its subgroups' included | nothing: the listing reads them in full |
| Forgejo | `repos/issues/search?type=pulls&owner=<owner>` | each pull request, its reviews and its head's combined status |

Where GitHub's search finds more than 1,000, or the owner is a GitLab
user, each repository is listed on its own instead, at least a request
each. An owner is read once its first poll has listed its repositories;
until then the view says so and is not shown. Kept mode keeps one record
per pull request as for one repository, folds a delivery only of a member
the membership record holds and has not left, and closes the records of a
repository that left the selection. A kept record is seen at least as
strictly as the membership record now says.

### Visibility a listing states

A live read through a REST listing without a token, as GitHub without a
token and Forgejo give, takes every row as private. `listed_visibility =
true` on a view makes such a read take who may see the repository from
what its listing states of it instead: `base.repo.visibility` on GitHub,
GitHub Enterprise Server and GHE.com, or its `private` flag, and
`private` with `internal` on Forgejo, Codeberg's or self-hosted (ADR-0062).
It widens only those rows, and only to what the listing states: a
repository the listing calls internal stays internal. It never widens a
read with a token, which reads visibility itself or keeps every row
private, a GitLab listing, which states none, a kept record (ADR-0061
decision 5), or an owner's rows, which its membership record names.
`herald doctor` names each view that sets it.

### Conflicts

`conflicts = true` on a view predicts which open pull requests would
conflict (ADR-0063). It reads the paths each pull request's head changes,
at most 100 each, and each row's `overlaps` names the open pull requests
of its repository merging into the same branch whose heads change a path
in common, whether the view shows them or not. The forge's own
`conflict` state still puts a pull request on its author's turn.

| Forge | Changed paths |
|-------|---------------|
| GitHub with a token | in the same GraphQL request as each pull request, no request more |
| GitHub without a token, Forgejo | `pulls/<number>/files`, a request each pull request read in full |
| GitLab | `merge_requests/<iid>/diffs`, a request each, at most `most_pulls` |

A pull request whose branch or paths were not read overlaps none. Kept
mode keeps the paths in each record until its head moves, so it reads
them once a head; a record kept before the view predicted conflicts is
read in full once. `herald doctor` names what predicting conflicts adds
to each report's requests.

### Live or kept

A view reads each source live, from the forge each time, or kept, from
one record per pull request in the store (ADR-0061). `mode = "auto"`
reads a source kept where the store keeps pull requests and the source
takes webhooks, its own or its GitHub App's, and live otherwise; `live`
and `kept` say so for every source. A kept view on a store keeping no
pull requests, such as `markers`, is a mistake doctor names, and stops
the view.

A record is keyed by the host of its pages, its repository's id and its
number. It holds what a view reads, when its draft state, review, checks,
mergeability, merge queue state and head last changed, each reviewer's
latest verdict and each check's latest outcome on the head delivered
since the last full read, and how it closed.

Each webhook delivery about a pull request a kept view reads is folded
into its record:

| Forge | Deliveries |
|-------|------------|
| GitHub | `pull_request` (its merge queue's `enqueued` and `dequeued` too), `pull_request_review`, `pull_request_review_comment`, `issue_comment` on a pull request, `check_run`, `status`, `repository` |
| GitLab | `Merge Request Hook` (an approval is its actor's), `Pipeline Hook` (the head's one check, `pipeline`), `Note Hook` on a merge request |
| Forgejo | `pull_request`, `pull_request_approved`, `pull_request_rejected`, `pull_request_comment`, `issue_comment` on a pull request, `status`, `repository` |

A delivery older than the record leaves it as it is. A comment moves only
the last activity. A review counts as GitHub's does: a change request
from an owner, member or collaborator outweighs an approval, and a
verdict of unknown weight, as Forgejo's deliveries give, reads
`review-unknown`. GitHub's `check_suite` is not folded: its check runs
carry each outcome. A delivery of another repository is refused.

Each round, or at most once each `reconcile`, kept mode lists the
source's open pull requests, each page conditional on its last answer.
Where a page changed, a pull request herald keeps no record of, or whose
head or last activity differs, which closed, or whose record was never
read in full, is read in full, and one kept open and no longer listed is
closed, how unknown. A record whose checks are still running is read in
full every listing, changed or not, as checks end with no activity of
their own. Reading in full takes one GraphQL request
for up to 100 pull requests on GitHub with a token, the listing itself on
GitLab, and each one's reviews and checks elsewhere, at most
`most_pulls` a round; the rest are kept as `unknown` and read on a later
round.

Who may see a kept pull request only narrows without a full read. A
delivery never widens it: a record a delivery begins is private until it
is read in full, and a `repository` delivery making a repository private
narrows every record of it. What a REST listing's `base.repo` names of
the repository's visibility narrows every record of it and widens none;
its rows read as private, as live mode reads them. Only a full read
through GitHub's GraphQL widens a record. GitLab names no project
visibility, so its records stay private.

`[store] keep_pulls` (`"30d"`) bounds how long a closed pull request is
kept; housekeeping forgets older ones. `herald doctor` names each source
a view reads kept, and how often it is listed.

## Reports

| Key | What it sets | Where absent |
|-----|--------------|--------------|
| `view` | the view it shows | `queue` |
| `every` | how long each window lasts within a day, `"1m"` to `"1d"` | `"1d"` |
| `at` | the wall-clock time a day's first window begins, `"HH:MM"` | `"09:00"` |
| `days` | the days windows begin on: `mon` to `sun` | every day |
| `zone` | the IANA time zone `at` and `days` are read in | `"UTC"` |

A window begins at `at`, then each `every` after it until the day ends. A
report's name is a source a route names: no source, app, other report or
rule may share it.
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

## Boards

A route with `living = true` keeps one post of each report it takes: the
first window posts it, and each window after edits it where it renders
otherwise (ADR-0056). Its identity is `report/<name>`, with no window. A
living route pins its post where it says nothing of `pin` and its
destination pins; `pin = false` leaves it unpinned. A living route names
only reports among its sources. Where its destination cannot edit a post,
as a Slack incoming webhook cannot, each window is posted anew and doctor
says so.

A `[boards.<name>]` table is a report and a living route to it at once:
it takes a report's keys and a route's but `id`, `sources` and `living`,
which it gives itself. Its route's id and its report's name are the
board's name, and it renders `herald:queue` where it names no template.

```toml
[boards.queue]
view = "review"
every = "1h"
destination = "discord"
channel = "1566200000000000100"
private = true
```

## Rules

A `[rules.<name>]` table watches a view once a window, as a report does,
and nudges each open pull request that waited past one of its `stages`:

| Key | What it sets | Where absent |
|-----|--------------|--------------|
| `stages` | how long a pull request waits before each nudge, 1 to 16 lengths such as `["2d", "7d"]` | no nudges |
| `most` | the most pull requests it nudges, and the most re-runs it plans, a window; the rest wait for the next window | `20` |
| `flaky` | `{ rerun = true }` plans a re-run of each flaky check (below); `mode` is `"plan"`, the only mode a rule takes; `runs` is the most failed runs on a head a plan reads, the newest first, 1 to 100 | no plans; `runs = 5` |
| `view`, `every`, `at`, `days`, `zone` | as a report's | as a report's |

A pull request's spell of waiting begins when anyone last acted on it, as
the forge's last update says, the clock `stale` reads. Each stage fires
once a spell, at the longest stage the pull request reached: its identity
is `rule/<name>/<host>/<repository>/pull/<number>/<spell start>/<stage>`.
A push, a review or a comment begins a new spell.

The `nudge` event carries `rule`, `stage`, `turn`, `reason`, `waited`,
`spell`, `repository`, `number`, `bot`, and `author` and `waiting_on`
where the forge names them, and links the pull request as `pull`. It
names the logins whose turn it is, its author's or each reviewer's asked,
without mentioning anyone. A nudge of a private or internal repository
reaches only private routes while `[doctor] private_to_public` refuses.
herald ships `herald:nudge`; a `[templates."herald:nudge"]` table replaces
it.

```toml
[rules.waiting]
view = "review"
stages = ["2d", "7d"]
every = "1h"

[[routes]]
id = "nudges"
sources = ["waiting"]
destination = "discord"
channel = "1566200000000000200"
template = "herald:nudge"
private = true
```

A rule names `stages`, `flaky`, or both.

### Check triage and flaky checks

Every board names each check failing on the pull requests it shows, those
failing on the most first (ADR-0065). Where a check fails on `broken_at`
of them (`3`) the base they merge into reads as broken. Kept mode records
each time a check failed and then passed on one head, from a delivery or
a read of the same head; a check that did so `least` times (`1`) within
`window` (`"7d"`) across the view's pull requests is a flaky candidate. A
view reading a source live tells none.

```toml
[views.ci]
mode = "kept"
broken_at = 3
flaky = { window = "7d", least = 1 }
```

A rule with `flaky = { rerun = true }` plans a re-run of each flaky check
failing on its view's pull requests, at most `most` a window. A plan names
the check, the head it fails on, how often it flipped, and each failed run
on the head that ran it with the exact call that re-runs its failed jobs.
A plan reads the newest `runs` failed runs: one request for them and, on
GitHub, one for each run's jobs. Every request a plan, a view or a
reconcile makes is charged to the run's budget as it is made, so a host's
request limit stops the read before the request past it:

| Forge | Runs read | The call |
|-------|-----------|----------|
| GitHub | `actions/runs?head_sha=<head>&status=failure`, then each run's jobs | `POST …/actions/runs/<id>/rerun-failed-jobs` |
| GitLab | `pipelines?sha=<head>&status=failed` | `POST …/pipelines/<id>/retry` |
| Forgejo | none: it has no API to re-run a run | none; the plan says to re-run it by hand |

The `rerun` event carries `rule`, `check`, `head`, `flips`, `repository`
and `number`, with `run_id` and `call` where a run can be re-run and
`reason` where none can, and links the pull request as `pull` and the run
as `run`. It is announced once a run and head, to the routes naming the
rule that say `private = true` alone. herald ships `herald:rerun`. A rule
only plans: `mode = "apply"` is refused, as a re-run is made by an
operator signed in through the forge, never by a rule on its own.

### Capacity

A view with `overviews = ["capacity"]` shows, beside its rows, the CI runs
queued and running in its repositories, the oldest queued, and the runs
started for a pull request on a commit no open pull request's head is,
superseded by a newer push (ADR-0066):

| Key | What it sets | Where absent |
|-----|--------------|--------------|
| `overviews` | the overviews it shows: `"capacity"` | none |
| `capacity_from` | `"shown"`, the repositories it shows a pull request of; or `"members"`, its owner's newest pushed | `"shown"` |
| `capacity_repositories` | the most members `"members"` reads, 1 to 100 | `20` |

| Forge | Runs read | Its token needs |
|-------|-----------|-----------------|
| GitHub, GitHub Enterprise Server, GHE.com | `actions/runs?status=queued` and `?status=in_progress` | Actions: read (a GitHub App or fine-grained token), or `repo` (`public_repo` for a public repository) as a classic token |
| GitLab, self-managed included | `pipelines?status=pending` and `?status=running` | `read_api` and the Reporter role |
| Forgejo | `actions/tasks`, those waiting and running | read access to the repository |

It reads and shows only repositories the view's `repositories` filter
holds: `"shown"` takes those of the rows the view shows, and `"members"`
only the members the filter holds. A listing that fills its page (100,
or 50 on Forgejo) is named in `view.overviews.unread` as holding more
than it counts, and the run's request budget ending leaves the section
unread with that reason rather than stopping the report.

Each repository costs two requests on GitHub and GitLab and one on
Forgejo, and `"members"` one more for their order: GitHub's `sort=pushed`
and GitLab's `order_by=last_activity_at`. Forgejo lists no push order, so
it takes the first members its membership record keeps. A view with a
`repositories` filter takes the first members it holds by name instead,
with no request for their order, so the push times of the repositories
it hides choose nothing. With `"shown"`,
the runs of a repository with no open pull request are not counted, and
doctor says so. A repository whose runs the token may not read, or a
Forgejo that answers no tasks API, is named in `view.overviews.unread`
with why, never counted as idle. Each run is seen as its repository's
rows are, or as its membership record holds it, or as private, and an
audience is shown only the runs and unread repositories it may see.

Templates read `view.overviews.capacity` as `queued`, `running`,
`oldest_queued` and `superseded`, each `{ repository, id, url, sha,
created }`, absent where the view shows no capacity; and
`view.overviews.unread` as `{ repository, reason }`.

```toml
[views.ci]
overviews = ["capacity"]
capacity_from = "members"
capacity_repositories = 20
```

Reports and rules watching one view read it once a round.

### Advisories

A view with `overviews = ["advisories"]` shows, beside its rows, its
repositories' open security alerts, to routes that say `private = true`
alone (ADR-0067):

| Key | What it sets | Where absent |
|-----|--------------|--------------|
| `advisories_from` | `"owner"`, the organization's or group's own listings and a user's members one by one; `"shown"`, the repositories it shows a pull request of; or `"members"`, its owner's newest pushed | `"owner"` |
| `advisory_repositories` | the most repositories read one by one, 1 to 100 | `20` |
| `most_advisories` | the most open advisories read, a page of 100 a request, 1 to 10000 | `1000` |

A one-repository source reads its repository whatever `advisories_from`
says. An owner's own listings keep only the advisories of repositories
its membership record holds. A GitHub user and a GitLab user namespace
have no such listing, so their newest pushed members are read one by one.
Only the repositories the view's `repositories` holds are read one by
one, and only their advisories are shown or count toward
`most_advisories`. A view with a `repositories` filter never reads its
owner's own listings, whose pages count the alerts of the repositories
it hides too: it reads each repository it holds one by one, the first
by name, at most `advisory_repositories`, with no request for their push
order, so what it shows is the same whatever the repositories it hides
hold or when they were pushed to. Where a held repository's advisory was left out past
`most_advisories`, or a listing names a next page off the forge's API,
the section says more were open, never how many; where an owner's
listing runs past the pages `most_advisories` can fill, that listing is
named unread with how many pages were read. Members past
`advisory_repositories`, and members with no open pull request where it
reads those shown alone, are named unread with how many were not read.
A report round sends every report's posts to the routes no section can
reach first, then reads advisories, only where a route can be shown
them, leaving one request for each route still waiting, then sends
those: the run's budget ending during the read names them unread and
never stops the report or delays another route's post.

| Forge | Listings read | Its token needs |
|-------|---------------|-----------------|
| GitHub, GitHub Enterprise Server, GHE.com | `code-scanning`, `dependabot` and `secret-scanning` `alerts?state=open`, of each repository or under `/orgs/<owner>` | Dependabot alerts, Code scanning alerts and Secret scanning alerts: read (a GitHub App or fine-grained token), or `security_events` (and `repo` for a private repository) as a classic token; an organization's listings also need an organization owner or security manager, with `read:org` as a classic token |
| GitLab, self-managed included | the GraphQL `vulnerabilities` of each project or group, detected or confirmed | `read_api`, the Developer role, and GitLab Ultimate |
| Forgejo | none: Forgejo raises no security alerts | |

A listing the token may not read, or one the forge does not find, is
named in `view.overviews.advisories.unread` with the permission it needs,
never shown as clean. A secret scanning row names the secret's type, its
repository, state, age and link, and never the secret or where it was
found: GitHub's listing is asked with `hide_secret=true`, and GitLab's is
asked no vulnerability's file.

**Who is shown them.** One rule decides: only a route that says
`private = true`, whose place herald checked is not publicly readable
right now, or that you vouch for where herald cannot check, is shown the
section. herald's own ActivityPub actor, a herald feed, a push to a peer
taking no private events, a Mastodon status posted `public` or
`unlisted` and a Discord crosspost post where anyone may read them, so
such a route saying `private = true` does not load. Before each run's
first advisory send, once a run for each place and charged to the run's
budget, herald asks:

| Destination | It is private only where | Asked by |
|-------------|--------------------------|----------|
| Matrix | joining takes an invite or a knock (`m.room.join_rules` `invite`, `knock`), or a membership of at most five rooms that each take one (`restricted`, `knock_restricted`, whose `allow` list is read one level deep), and the history is not `world_readable` | the room's two state events and the join rules of each room it lets in, through the bot, which must have joined each |
| Slack | the channel is private and shared with no other organization through Slack Connect; a channel an Enterprise Grid organization shares counts as public unless the destination says `org_shared = "private"` | `conversations.info`, through a bot token with `groups:read` (`channels:read` for a public channel) |
| Discord | the server's `@everyone` may not view the channel, every other role that may is listed by id in the route's `viewers` (Administrator roles and the bot's own role included), no member is let in by an overwrite of their own unless listed too, and the server's onboarding neither shows the channel nor hands out a listed role; a thread is not read | the channel, its server and its onboarding, through a bot token that sees the channel |
| Mastodon, `visibility = "private"` | the account approves each follower (`locked`) | `verify_credentials` |

A place anyone more may read, or one herald cannot read, budget ended
included, is sent the line below, and a living post there loses the
section at its next post. A Discord or Slack route reached by a webhook
cannot be asked: it is sent the line unless it also says
`vouch_unchecked = true`, your word that its place is private. That key
loads only beside `private = true`, and only on a route herald cannot
check; doctor lists every route sent advisories on it. Doctor's online
check reads a Matrix room and a Discord channel the same way, and names
a private route to a place anyone more may read a mistake, naming each
Discord role or member that withholds it by id and name. No setting widens
it, `[doctor] private_to_public` included, and no repository's
visibility does, a public repository's advisories included. Every other
route is sent, in its place, the one line "A section of this report was
withheld from this route because it is not private.", the same where the
view found none, so the line says nothing of what was held. A route's
counts, titles and totals never count advisories, so what it is sent is
the same, byte for byte, whatever the section held. Doctor flags two
routes posting to one place where only one is shown the section.

**Only living posts.** A report carries its advisories only in a living
post (`living = true`, or a board, on a destination that edits), which
herald can always find again to take the section out. A private route
that posts each window anew is sent the withheld line, and doctor says
the report must be living to carry advisories.

**Living posts.** Before a living post is sent the section, the report's
cursor records where the post is, never what it showed, and one store
record names the report, its name alone; where either cannot be written,
or the store keeps no such record (the markers store), the post is sent
the withheld line, and doctor says why. Once its route is gone, stops
being living, stops naming the report, moves, or is no longer shown the
section, or the report, its board or its view is gone from the config,
or the view no longer reads advisories, herald edits the post at the next report round to "The advisory section
of this report was removed because this route no longer receives it."
(the template `herald:recalled`, which cannot be replaced and says
nothing else). Where its destination is gone or cannot edit, or no route
reaches it, herald retries each round and doctor names the post, which
is then deleted by hand. Before anything else a run sends, it keeps two
requests for each recorded post it may recall, so the post itself goes
and the fixed line is posted however long the section was; a run whose
budget ends before even that leaves the section up until a run reaches
it, and doctor names such a post. Each run also checks every such post's place
again and recalls the post where it no longer passes. Matrix and
Mastodon keep what an edit replaced (a room's timeline holds each
`m.replace`, a status shows its edit history), so there a living post of
a post that is or was sent the section is never edited: each new
revision, and a recall, redacts or deletes every event herald sent for
it, the post itself first, and posts anew, under a key the homeserver or
instance has not seen, so it never answers with the deleted post; an
edit, a redaction and a post outside a replacement each go under a key
of their own too. A replacement cut short is resumed, and a post it sent
but never recorded is read back first. A new replacement reads back
what earlier runs sent and never recorded and deletes it; a post it
cannot find again keeps its recall record, and doctor names it as
possibly orphaned, to be deleted by hand. `herald orphans clear
<report>` lists them by route (`--route <id>` for one route), each
route's with a short digest of what it lists and the command that
clears it, and says what clearing means. `herald orphans clear <report>
--route <id> --deleted --seen <digest>` says that route's parts were
deleted by hand and drops them, only while they are still the ones
listed (else it refuses: list them again), and the next round drops the
record. A post, chunk or edit sent again after its answer was lost goes
under the key it was first sent under, so the server never makes it
twice.
Between runs nothing reacts, and what a reader already saw cannot be
unsent.

**What a run tells and when it sends.** A run tells of a route that is
or was sent the section, and of each recall, in one fixed line: sent, or
stopped, never why, its steps, how many parts it took or whether the
post changed. Where such a route stops, the next run sends that route
alone; every other route's window is remembered. Every route no section reaches is sent its post before any
advisory is read, any post recalled or any section sent, so neither what
it is sent nor whether the run's budget lets it out depends on the
advisories.

| Where a view leaves herald | Advisories |
|----------------------------|------------|
| A route's post, or a living post's edit | in a living post alone, where the route says `private = true` and its destination does not post publicly |
| `herald queue`, as a table or JSON | with `--private` alone |
| A grouped route's digest, a herald feed, the static log, a push route | never: they carry no view |
| A rule's nudges and re-run plans | never: they show no view |
| A run's told lines, a round's answer, doctor, the app API | never |

`herald queue --private` has no setting or environment variable: it is
typed each time, and in a public CI job's log it publishes the
advisories.

Templates read `view.overviews.advisories` as `count`, `more`, `open`,
each `{ repository, tool, severity, summary, state, url, opened, age }`,
and `unread`, each `{ repository, reason }`, only where the route was
shown them; and `view.overviews.withheld`, the line, only where it was
not. The shipped `herald:queue` template prints both.

```toml
[views.security]
overviews = ["advisories"]
advisories_from = "owner"
```

## Templates

A route's template reads the board as `view`:

| Key | Holds |
|-----|-------|
| `view.name`, `view.total`, `view.stale` | the view's name, how many it shows, how many are stale |
| `view.counts` | each state with its `count`, as `{ state, count }` |
| `view.groups` | each group's `name` and `rows` |
| `view.batches` | each batch's `package`, `to`, `green`, `failing` and `rows` |
| `view.overviews` | its overviews, `capacity` and `unread` (above) |
| `view.failures` | each failing check's `check`, `count`, `broken`, `flips`, `flaky` and `pulls`, each `{ repository, number }` |
| a row | `project`, `repository`, `number`, `title`, `url`, `author`, `bot`, `draft`, `labels`, `reviewers` (each asked, or a team's `org/team`), `opened`, `updated`, `state`, `reason`, `waited` (as `3d`, `5h` or `12m`), `waited_seconds`, `stale`, `waiting_on` (the logins whose turn it is), `overlaps` (the numbers of the open pull requests changing a path it changes, where the view predicts conflicts) |

herald ships `herald:queue`; a `[templates."herald:queue"]` table replaces
it.

## `herald queue`

```sh
herald queue                 # the built-in view, as a table
herald queue review --format json
herald queue security --private   # this output is private: print its advisories
```

The JSON holds the keys a template reads as `view`. A view's advisories
are printed only with `--private`; without it they are withheld, as from
a route that is not private.

## Time zones

The time zone database is bundled into every build, which adds about
207 KB (49 KB compressed). A build without `herald-core`'s default `tzdb`
feature reads only `UTC`.
