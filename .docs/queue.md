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
| `areas` | globs, ignoring case, of any one area its sources name (below) | every one |
| `stale` | how long one waits with nobody acting before it is stale | `"14d"` |
| `stale_only` | whether it shows only the stale | `false` |
| `group_by` | `turn`, `repository`, `author`, `area` or `nothing` | `turn` |
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

Each pull request is read with the branch it merges into, the branch its
head is on and the title of its milestone, where its forge names them, at
no request of its own: a template reads them as a row's `base`,
`head_branch` and `milestone`, and `herald queue` names the milestone. A
row is seen as its repository is, so its milestone is too. Kept mode takes
them from each delivery of a pull request, a milestone set or cleared
included. GitLab's merge request delivery names a milestone by its id
alone: a kept record whose milestone differs from what a listing names is
read again, so a kept GitLab record takes its milestone from the next
listing.

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

### Merge queue

`queue = true` on a view reads where each pull request stands in its
forge's merge queue or merge train (ADR-0078): its position, its state
(`queued`, `checking`, `mergeable`, `merging` or `unmergeable`), when it
entered and the forge's estimate in seconds. Each row's `queue` holds
them, and `left` when and why it last left, where a webhook said. A
pull request in the queue is `queued`; one that left is told by the
other rules.

| Forge | Queue entries |
|-------|---------------|
| GitHub with a token | `mergeQueueEntry` in the same GraphQL request as each pull request, no request more |
| GitHub without a token | none: only GraphQL names the merge queue |
| GitLab | `merge_trains?scope=active`, a request each project a round; a project whose trains its token may not read, as without Premium, answers 403 and shows none, but a 403 for the rate limit stops the read |
| Forgejo, Gitea | none: they have no merge queue |

Without `queue = true` GitHub's `isInMergeQueue` still puts a pull
request in the queue, with no position or state. A GitHub Enterprise
Server too old to name `mergeQueueEntry` refuses a query asking it:
leave `queue` off there. `herald doctor` names each source's queue read
and its cost, or why it reads none.

The entry is part of its pull request: a private or internal
repository's position, state and timing are shown, withheld and counted
exactly as its row is.

### Areas

A source names the areas of its repositories by path globs (ADR-0071):

```toml
[sources.monorepo]
kind = "github"
repository = "acme/monorepo"

[sources.monorepo.areas]
web = ["web/**", "packages/ui/**"]
docs = ["docs/**", "*.md"]
```

An owner's override, `[sources.<name>.repositories."<glob>".areas]`,
gives the repositories it names their own areas. A view reading such a
source reads the paths each pull request's head changes, as predicting
conflicts does, and each row's `areas` names the areas one of its paths
falls in. A view's `areas = ["web"]` shows only pull requests in one of
them, each glob matching an area its sources name, and
`group_by = "area"` groups rows by area, a pull request in several
areas in each of their groups. A table holds at most the source's
`most_areas` areas (200 where absent, at most 2000) and 1000 globs; a
changed path longer than 1024 characters falls in no area. A
private repository's areas are withheld with its rows.

Each row's `areas` in JSON names each area with its kind: `paths` for a
table's area, and the kinds below for CODEOWNERS.

#### Areas from CODEOWNERS

`areas = "codeowners"` reads each repository's CODEOWNERS file in place
of a table (ADR-0073), as its forge reads it:

```toml
[sources.monorepo]
kind = "gitlab"
repository = "acme/monorepo"
areas = "codeowners"
```

Each owner a line names for a changed path is an area: a team
(`@org/team`, kind `team`), a GitLab group (`group`), a role
(`@@developer`, `role`) or a login (`person`), and so is each GitLab
section (`section`). An email is never an area. A GitLab top-level
`@name` may be a group or a person, which the file does not tell: herald
looks each up once a run, at most 50 names a file, and one it cannot tell
is of kind `name`. A login is an area of one person, so its JSON kind
`person` tells a consumer a person's group from a team's.

| Key | Where | Default |
|---|---|---|
| `codeowners` | source, override | the forge's own places in its own order: GitHub `.github/CODEOWNERS`, `CODEOWNERS`, `docs/CODEOWNERS`; GitLab `CODEOWNERS`, `docs/CODEOWNERS`, `.gitlab/CODEOWNERS`; Forgejo `CODEOWNERS`, `docs/CODEOWNERS`, `.gitea/CODEOWNERS`, `.forgejo/CODEOWNERS`; at most 8 |
| `codeowners_ref` | source, override | the branch the forge applies: `"base"`, the branch the pull request merges into, on GitHub and GitLab; `"default"`, the default branch, on Forgejo. Gitea applies `"base"`: set it there |
| `most_codeowners` | source | `"128KiB"`, at most `"3MiB"`; a longer file is read as none |
| `most_areas` | source | `200`, at most `2000`; a table naming more is refused, a file naming more is read as none |

The first place a file is found at is read, once a round per repository
and branch, through the source's token and API base; where the branch a
pull request merges into is not known, the default branch is read. A
file naming more than `most_areas` areas is read as none, and `herald
doctor --online` says so and names the setting. Where the store keeps
pull requests, each file read is kept there with the validator its forge
answered with (ADR-0074): the next round asks the place it was found at
conditionally, and a file unchanged since costs a `304` answer and is
neither downloaded nor read again. A kept file read under other bounds,
or one the store cannot read, is read again in full. As the names are known
only as each file is read, a view's or a route's `areas` of such a source
are taken as written. `herald doctor` names where and at which branch
each source reads, and `herald doctor --online` names each line of a
repository source's file herald skips, by number and why, a file it
reads none of, and a file the store would keep no record of (its forge
gives no validator, or the record would be longer than
`most_codeowners`), which is read in full every round (ADR-0076). A file that cannot be read stops the view, named by its
source alone unless the output is said private.

### Teams and load

`load = {}` on a view shows its load beside its rows (ADR-0075): per team
and per area, never per person, the review requests open now on the rows
it shows, and the reviews given on them over `window` where every source
it reads is read kept (a live read keeps no review, so it shows requests
alone). A team is a top-level `[teams.<name>]`, named as a review
request and a CODEOWNERS file name it:

```toml
[teams."acme/platform"]
members = ["ada", "grace", "linus@gitlab.example.com"]

[views.queue]
load = { window = "14d" }
```

A request naming `acme/platform` counts for that team, and a request
naming a login counts for every team holding it; a member written
`login@host` counts only on that forge's host. Each area a row falls in
counts every request open and review given on it.

A row of fewer than `least_team` people could be one person's, so it
folds into "other": a team of fewer members, an area naming a person, a
GitLab `@name` no lookup told apart, and a team, group or role area whose
`[teams.<name>]` of the same name has fewer members. A team, group or
role area no `[teams.<name>]` gives members to folds too, unless
`unknown_teams = true`, which shows it as it is though it may be one
person; a person's area folds whatever it says. Path and section areas
always show. "other" shows only where the people folded into it, known
by name, are `least_team` or more; otherwise there is no "other" row,
and `herald doctor` says why and names each team below the number.

The load is counted from the rows each surface shows: `herald queue`
from those it prints, so nothing of a private or internal repository is
counted without `--private` or `[cli] private`, and a report from the
rows each route is shown, so a route shown only public repositories'
rows is shown their load alone.

A forge source may also read its forge's own teams (ADR-0077):

```toml
[sources.web]
kind = "github"
owner = "acme"
teams = "forge"
```

Each round a view showing its load reads the source, it reads at most
100 teams of the organization or group owning the source's
repositories, each with at most 500 members, under the source's own
`api`, so GitHub Enterprise Server, GHE.com, GitLab self-managed and
Gitea are read where the source points. GitHub's and Forgejo's teams are
named `org/team`; GitLab's are the group and each group under it, by
full path. A team the config names keeps the config's members, and a
forge team folds below `least_team` as a config team does. Members'
logins are held for the round alone, to tell which team a request counts
for, and are never shown or kept. A forge team reaches only a route that
says `private = true` and `herald queue` with `--private` or
`[cli] private`, whatever repository a row is of: who is in an
organization's teams is the organization's, and can be secret. The token
needs Members: read (a GitHub App or fine-grained token) or read:org (a
classic token) on GitHub, read_api on GitLab, and read:organization on
Forgejo and Gitea; a forge that refuses leaves the load to the config's
teams, and `herald doctor --online` names the permission. JSON and a template read
`view.overviews.load`, with `teams` and `areas` (each a `name`, an
area's `kind`, `requested` and `reviewed`) and `other`; `reviewed` is
null where reviews given were not counted.

| Key | Where | Default |
|---|---|---|
| `[teams.<name>] members` | top level | none; logins, or `login@host`, at most 500 a team and 200 teams |
| `load` | view | off; `{}` shows it |
| `load.window` | view | `"7d"`, from `"1d"` to `"90d"` |
| `load.least_team` | view | `3`, from `3` to `50`; fewer is refused |
| `load.unknown_teams` | view | `false` |

### Release readiness

`release` on a view picks the open pull requests of one release and
says whether it is ready (ADR-0081). One selector is enough:

```toml
[views.v2]
release = { milestone = "v2.0" }
```

| Key | Default |
|---|---|
| `release.milestone` | none; a glob on the milestone title a pull request carries |
| `release.label` | none; a glob on any label it carries |
| `release.branch` | none; a glob on the branch it merges into |
| `release.pull` | none; `"release-please"`, `"changesets"`, `{ head = "<glob>" }` or `{ title = "<glob>" }` |
| `release.combine` | `"all"`: each selector named picks it; `"any"`: one does |
| `release.blocker_labels` | `["blocker", "release-blocker"]`, globs, at most 50 |
| `release.blocked_by` | every one of `"label"`, `"failed"`, `"draft"`, `"changes"`, `"base"` |
| `release.header` | `true`: the board is headed by the open milestones `milestone` matches; it needs a milestone selector |
| `release.rows` | `"pulls"`: the board lists the pull requests it picks; `"issues"`: the open issues of the open milestones `milestone` matches; it needs a milestone selector |
| `release.withheld` | `"unknown"`: a surface some of its pull requests are withheld from is told the verdict is not known; `"shown"`: judged by those shown; `"masked"`: the whole verdict, a withheld hold named private |

At least one of `milestone`, `label`, `branch` and `pull` is named. The
view shows only the pull requests its release picks. A pull request
holds the release back for each blocker of `blocked_by` it meets: it
carries a blocker label, a check failed, it is a draft, or a reviewer
asked for changes. One listed past `most_pulls` holds it back whatever
`blocked_by` says, since its checks and reviews are not known. With
`"base"`, each branch a pull request of the release merges into holds it
back where a check failed on its head, or where its checks could not be
read (ADR-0083).

A surface may be shown only some of a release's pull requests: a public
route of a report is not shown a private repository's, nor is
`herald queue` without `--private`. There `withheld` says what it is
told. By default a pull request shown that holds the release back makes
it not ready, saying nothing of those withheld; where every one shown is
clear, the verdict is not known: the table says `release: not known,
some pull requests are private`, and nothing says which, how many or
why. `"shown"` judges the release by the pull requests shown alone,
and the table says `release: ready, by the pull requests printed`.
`"masked"` tells the whole verdict, each withheld pull request holding
the release back one hold saying only that it is private, `(private)` in
the table. A template reads `view.readiness`:

| Key | What |
|---|---|
| `ready` | whether nothing holds it back; none where that is not known |
| `holds` | each `{ private, reason, repository, number, url, branch, named, more }`: `reason` one of `label`, `failed`, `draft`, `changes`, `base`, `unread`; a pull request's names its `number` and `url`, a base branch's its `branch`; `named` the blocker labels or failed checks, at most 10, `more` past those, or why a base branch was not read; a private one's keys are none or empty |
| `more` | the holds past the 100 named |

With a `milestone` selector the board is headed by each open milestone
the glob matches, in each repository it shows a pull request of the
release in, and a one-repository source's own (ADR-0082). On an owner
source the glob spans every member the view holds, and with
`milestones.group = true` on a GitLab owner also the group milestones of
each group at or under the owner above those members, each group read
once (ADR-0086). `release.repository = "<owner>/<name>"` holds the
release to that one repository: its pull requests, milestones and issues
alone, and no group. One request a round a repository or group, at most
20, and on GitLab one more a milestone to count its issues. Public
repositories are read first, then internal, then private, and within
each those with a pull request of the release first, so the 20 are never
spent on what a public route may not see. Each is seen as its
repository: a public route, and `herald queue` without `--private`, are
shown the public repositories' alone, and learn nothing of a private or
internal member, not even that it exists, beyond what `withheld` tells.
Everything read from a group, its milestones, their counts and their
issues, is seen as private: a group milestone covers every project of
the group, including those the source does not list, so none is known to
be public. A repository two reads see apart is seen as the stricter. A repository or
group past the 20, or whose milestones could not be read, is named with
why. A template reads `view.overviews.header`:

| Key | What |
|---|---|
| `milestones` | each `{ repository, title, url, due, overdue, open, closed }`, at most 10 a repository; `due` a day, `overdue` whether it ended before today in UTC, `open` and `closed` its issues as the forge counts them, none where it does not |
| `unread` | each `{ repository, reason }` whose milestones could not be read, or wholly |

With `rows = "issues"` the board lists, in place of its pull requests,
the open issues carrying each open milestone the `milestone` glob
matches, read beside the header from the same repositories (ADR-0084):
one more request a round for each milestone matched, one page of the
source's `page`, at most 50 rows a repository. `header = false` keeps
the issues and leaves the milestones out. The release's pull requests
still judge whether it is ready. Each issue is seen as its repository,
a group milestone's issue as private, and a confidential GitLab issue as
private: a public route, and `herald
queue` without `--private`, are shown the public repositories' public
issues alone. A milestone whose issues fill the page, a repository past
the 50, and a listing the token may not read are named unread in
`view.overviews.header.unread`. A template reads `view.issues`, none
where the board lists pull requests:

| Key | What |
|---|---|
| `issues` | each `{ repository, number, title, url, milestone }` |

With `"base"` in `blocked_by`, the head of each base branch the
release's pull requests merge into is read once a round (ADR-0083): its
check runs and commit statuses on GitHub (two requests), its newest
pipeline on GitLab (and one more for its failed jobs where it failed),
its combined status on Forgejo, at most 20 branches a view. Where the
store keeps pull requests, each request is asked on the validators the
last answer came with, so an unchanged branch reads nothing again. A
branch with a failed check is one hold naming each failed check, `base
branch main of o/r failed: build, lint` in a report and `base main of
o/r failed build, lint` in the table; one that could not be read, refused,
rate-limited, past the 20 or past the run's budget, is one hold naming
why. Each is seen as its repository, and one withheld from a surface is
told as `withheld` says, as a withheld pull request is. A template reads
`view.overviews.bases`:

| Key | What |
|---|---|
| `bases` | each `{ repository, branch, state, failing, more, reason }`: `state` one of `passed`, `pending`, `failed`, `absent`, `unread`; `failing` the failed checks, at most 10, `more` past those; `reason` why it was not read |

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
| GitHub | `pull_request` (its merge queue's `enqueued` and `dequeued` too, with why it left), `merge_group` (`checks_requested` and `destroyed` move a waiting entry), `pull_request_review`, `pull_request_review_comment`, `issue_comment` on a pull request, `check_run`, `status`, `repository` |
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
| `dequeued` | `true` alerts once on each pull request that left the merge queue for a failure of its own: `checks-failed`, `checks-timed-out`, `conflict`, `invalid` or `protections`; a list such as `["checks-failed", "removed"]` names the reasons, of those and `removed`, `cleared`, `merged` and `unknown` | no alerts |
| `release` | `{ ready = true }` alerts once the view's release turns ready; `{ late = 7 }` alerts once a milestone and due day on a release still held back 7 days, 0 to 365, before the due day of the milestone due first; both may be named; the view needs a release, and `late` needs its `header = true` or `rows = "issues"` | no alerts |
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

The `dequeued` event carries `rule`, `reason`, `failure`, `left`,
`repository`, `number`, `bot`, and `author` where the forge names it,
and links the pull request as `pull`. Why a pull request left is known
where its source is read kept, from GitHub's `dequeued` webhook; its
identity holds when it left, so each departure alerts once. It reaches
only private routes for a private or internal repository, as a nudge
does. herald ships `herald:dequeued`.

The `readiness` event alerts on the view's release, for each audience of
the rule's routes apart, as a report parts them: the private routes are
judged on every pull request, the others on the public repositories'
alone, beside the rest as `release.withheld` says, so each is alerted as
its board would tell it. A release turns ready when nothing holds it back
for that audience after another verdict told it before: the first verdict
a rule sees is only remembered, in its cursor, and a verdict that is not
known never fires. A late release is one known held back on or after
`late` days before the due day of the milestone due first that the
audience may see; it fires once a milestone title and due day, and once
the due day passed `days` is below zero. A public route is told nothing of
a private or internal repository: no title, no milestone, and no verdict
beyond what `release.withheld` lets it know. The event carries `rule`,
`view`, `verdict` (`ready` or `late`), `holds`, and for a late release
`milestone_title`, `due` and `days`, and links the milestone as
`milestone` where its forge names a page. herald ships
`herald:readiness`.

```toml
[rules.release]
view = "release"
release = { ready = true, late = 7 }
every = "1d"
at = "09:00"
```

A rule names `stages`, `flaky`, `dequeued`, `release`, or more than one.

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
on the head whose job of that check failed, with that job and the exact
call that re-runs that one job (ADR-0068). A plan reads the newest `runs`
failed runs: one request for them and one for each run's jobs. Every
request a plan, a view or a
reconcile makes is charged to the run's budget as it is made, so a host's
request limit stops the read before the request past it:

| Forge | Runs read | The call |
|-------|-----------|----------|
| GitHub | `actions/runs?head_sha=<head>&status=failure`, then each run's jobs | `POST …/actions/jobs/<job>/rerun` |
| GitLab | `pipelines?sha=<head>&status=failed`, then each pipeline's failed jobs | `POST …/jobs/<job>/retry` |
| Forgejo | none: it has no API to re-run a job | none; the plan says to re-run it by hand |

The `rerun` event carries `rule`, `check`, `head`, `flips`, `repository`
and `number`, with `run_id` and `call` where a run can be re-run and
`reason` where none can, and links the pull request as `pull` and the run
as `run`. It is announced once a run and head, to the routes naming the
rule that say `private = true` alone. herald ships `herald:rerun`. A rule
only plans: `mode = "apply"` is refused, as a re-run is made by an
operator signed in through the forge, never by a rule on its own.

### Applying a re-run

`herald rerun [rule]` lists each plan now, with the command that applies
it; it reads no write credential. `herald rerun <rule> --apply --job
<job>@<head>` re-runs the jobs named, each at most once, and only an
operator at the terminal can: apply never runs unattended. It re-runs
the one job of the flaky check, never the run's other failed jobs. It
needs, on the source that reads the repository:

```toml
[sources.app]
kind = "github"
repository = "acme/app"
token = { secret = "HERALD_GITHUB_TOKEN" }
write = { app = "rerun" }                  # or { token = { secret = "NAME" } }
sign_in = { client_id = "Iv23liExample" }
```

| Key | What it is | Default |
|-----|------------|---------|
| `write` | the credential the call is made with: `{ token = { secret } }`, or `{ app = "<github_apps name>" }` with `installation` | none: apply refuses |
| `sign_in.client_id` | the GitHub App, OAuth application or GitLab application the operator signs in through, by the forge's device flow; never a write App's own. A private GitHub repository needs a GitHub App's client id: its user token holds that App's read permissions. A GitHub App must be installed on the repository's account and hold no permission but `read`, its account permissions among them | none: apply refuses |
| `sign_in.scopes` | the scopes asked for, each one that only reads: `read:user` or `read:org` on GitHub; `read_api`, `read_user`, `openid`, `profile` or `email` on GitLab. Any other does not load | none on GitHub, `["read_api"]` on GitLab |
| `sign_in.web` | the web root the device flow is asked at | the API's: `https://github.com`, a GHE.com tenant's `https://<tenant>.ghe.com`, a GitHub Enterprise Server's or self-managed GitLab's host |
| `sign_in.least` | the least role the operator must hold on the repository: `"write"` (GitHub write, GitLab Developer, or above), `"maintain"` (GitHub maintain, GitLab Maintainer) or `"admin"` (GitHub admin, GitLab Owner) | `"write"` |

The write credential is held apart. The config does not load where its
secret is one herald reads for anything else (the source's token, another
source's, a destination's, an App a source reads as, a webhook, the
static log's token, the store); where another source's `write` names it at another API; where a
source reads as its App, under any table; or where its App mints at
another origin than the source reads. An apply refuses a write secret
holding the read secret's value under another name, and a write App
that is an App herald reads as: the minted token's installation id is
compared with every installation a source reads through, and its App id,
as GitHub names it, with every App a source reads as, so the same App
under another table, with another key or on another account makes no
call. It is sent to the source's own API alone, never read by a poll, a
report or the server, and kept nowhere. A write App's token is minted
for one call, after every check and herald's record, holding `actions:
write` on that one repository alone: where GitHub's answer says it holds
more, or names another repository, it is never used and herald revokes it (`DELETE /installation/token`
with that token, over the write connection, whatever the budget holds),
saying in the refusal where the revocation failed. A stolen write credential re-runs without herald,
so give it nothing but re-running: a GitHub App installed with `actions:
write` alone on the repositories concerned, or a GitLab token of the
Developer role on those projects alone. The forge's audit log names that
credential, not the operator; herald's own record names them.

An apply reads the plans again and takes only the jobs they still hold
on the head named, so a plan listed before the head moved re-runs
nothing, and signs the operator in only at the sources of those jobs.
herald shows a code and the page to enter it at, a page on the sign-in's
own web root over https alone, and waits at the forge's interval until
they finish, decline or the code expires. Their token lives in memory
for that apply alone, is never written, logged or shown, and only reads.
On GitLab herald refuses it where a scope `/oauth/token/info` names can
do more. On GitHub what the token is decides, by the prefix GitHub
documents for it:

- `gho_`, an OAuth application's token: `GET /user` must name its scopes
  in `X-OAuth-Scopes`, and each must be on the read-only list; a token
  whose scopes GitHub does not name is refused.
- `ghu_`, a GitHub App's user token: herald reads every page of `GET
  /user/installations` with it, then `GET /apps/{app_slug}` once for each
  App they name; it refuses the token where an installation or an App
  holds a permission that is not `read` (an App's account permissions,
  such as `git_ssh_keys`, among them), an installation is the write
  App's or names another client id than `sign_in.client_id`, none names
  its App, or any of these reads fails, whatever the
  scopes header says.
- Any other token is refused, and the refusal shows no part of it.

When the apply ends, whatever came of it, herald asks GitLab to revoke
the token (`POST /oauth/revoke`, as a public client); GitHub revokes a
token only for a client holding its secret, which herald never holds. The
revocation is sent only where the apply ends as it should: on a panic or
a Ctrl-C it is not, and the token lives on. A GitLab token lives until it
expires (two hours), a GitHub App's user token until it expires (eight
hours, where the App has user token expiry turned on), and an OAuth
application's until the operator revokes it at the forge.

Right before each call herald reads, in turn: the operator's role on the
repository, the pull request's head, and the job (it must be of the run
named, have run on that head, read as failed, and neither it nor its run
been re-run, every page of a GitLab pipeline's jobs read). It then writes
its record of the re-run (run, job, head, operator, the credential's
name, time) as new, so two applies racing never both call, and only then
mints a write App's token and calls. A call whose answer is lost is not
asked again: herald reads the job back and records whether it was
re-run. Only a call the forge refused outright, or one never sent as the
run's budget ended or the App's token could not be minted or held more,
may be asked for again; where herald's record could not be updated, it
still says the call is being made, the apply says so, and herald never
asks for that job again. A store that keeps no records (`markers`)
refuses every apply. Forgejo has no API to re-run a job nor a device
flow, so its sources take neither key.

## `herald queue`

```sh
herald queue                 # the built-in view, as a table
herald queue review --format json
herald queue security --private   # this output is private: print its private rows and advisories
```

The JSON holds the keys a template reads as `view`. A private or
internal repository's pull requests, and its CI runs and unread lines in
the overviews, are printed only where the output is said to be private;
a repository whose visibility is not known counts as private. Without
`--private` they are withheld, and the table's last line and the JSON's
`withheld_pulls` count the pull requests withheld, never naming one.
`[cli] private = true` prints them by default:

```toml
[cli]
private = true   # false where absent
```

Set it only where the output stays private. `herald queue` run in a CI
job prints to the job's log, and a public repository's logs are public:
with the setting on, every private repository the view reads would be
named there, with its pull requests' titles and authors. A view's
advisories are printed only with `--private`, whatever `[cli] private`
says, as from a route that is not private.

A read that fails stops `herald queue` and `herald rerun`. Its error
names only the configured source and the kind of failure, such as
`the source "work" could not be read: its forge could not be reached;
--private, or [cli] private, prints why`. The forge's, the transport's
or the store's own words can name a repository's path, its numeric id
or a pull request, so they are printed only with `--private` or
`[cli] private`.

## `herald rerun`

```sh
herald rerun                       # every rule's plans, read now
herald rerun flaky --apply --job 420@<head>   # sign in, then re-run job 420 on that head once
```

Like `herald queue`, it withholds a private or internal repository's
plans unless `--private` or `[cli] private` says the output is private,
and counts the pull requests withheld. `--apply` reads the plans the same
way, so a job of a withheld plan is refused as not planned, saying that
`--private` or `[cli] private` would plan it, without naming it.

`--apply` prints, for each job named, whether it was re-run and why not
where it was not. Every line drops the control and format characters a
forge's names could hold, and a line or paragraph break becomes a space;
`herald queue` and `herald doctor` with `--format json` escape them as
`\uXXXX` instead, so the JSON reads back unchanged.

## Time zones

The time zone database is bundled into every build, which adds about
207 KB (49 KB compressed). A build without `herald-core`'s default `tzdb`
feature reads only `UTC`.
