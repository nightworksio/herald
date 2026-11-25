# Merging: the forge's merge queue or herald's own

A merge queue merges the pull requests a writer queues, one base branch
at a time, each only once it is green on the base it lands on
([ADR-0087](decisions/0087-a-pull-request-a-writer-queues-goes-to-its-forge-s-merge-queue-or-herald-s-own-proves-it-on-its-base-and-merges-it.md)).
By default herald hands each pull request to the forge's own merge queue
or merge train. Where the forge has none, as on Forgejo, Gitea, GitHub
personal repositories or GitLab without merge trains, herald runs a
serial queue of its own on the branches that ask for it.

```toml
[sources.app]
kind = "github"
repository = "acme/app"
token = { secret = "HERALD_GITHUB_TOKEN" }

[merge_queues.app-main]
source = "app"
branch = "main"
runs = "herald"
method = "squash"
write = { app = "merger" }        # or { token = { secret = "NAME" } }
```

`herald setup merge-queue [name]` asks for each key in turn, with its
default chosen and what the forge says shown beside it (whether the
branch has a queue; a method the forge refuses is not offered), offers
`runs = "herald"` where the forge has no queue on the branch, and writes
the table into the config once you confirm it, editing the queue of that
branch where one is written already. A flag answers each question
(`--branch main --method squash`), `--yes` takes the default of every
question no flag answers, and `--print` prints the table in place of
writing it. It names the secret the write token is kept under, never the
token, and starts no queue: `herald merge-queue` reads it next.

| Key | What it is | Default |
|-----|------------|---------|
| `source` | a source of kind `github`, `gitlab` or `forgejo` | required |
| `repository` | the repository, where the source is an owner | the source's own |
| `branch` | the base branch the queue merges into; one table per branch | required |
| `runs` | `"forge"`: hand each pull request to the forge's merge queue or merge train; `"herald"`: herald's own serial queue, also where the forge has one | `"forge"` |
| `enter` | `{ label = "<name>" }`, or `{ comment = "<command>" }`: a comment whose whole body is the command, or the command and a commit's full id | `{ label = "herald:merge" }` |
| `least` | the least role the person who queues holds: `"write"` (GitHub write, GitLab Developer, Forgejo write, or above), `"maintain"` or `"admin"` | `"write"` |
| `vouches` | which head a gesture vouches for: `"gesture"`, `"seen"` or `"approved"` (see [which head a gesture vouches for](#which-head-a-gesture-vouches-for)) | `"gesture"` |
| `skew` | how far apart the forge's stamps of a gesture and of a push may be: a push stamped up to this before a gesture, or before a GitLab approval's note, counts as after it, so the gesture never vouches for a head that push may have brought | `"5s"` |
| `method` | `"merge"`, `"squash"`, `"rebase"`, `"fast-forward"`, or `"repository"`: the repository's one allowed method | required: a queue without it does not start |
| `message` | `"forge"`: the forge's own commit message for the method; `"pull"`: the pull request's title and body, with `"merge"` or `"squash"` | `"forge"` |
| `may_fail` | checks that may be red or missing on this branch; never one the forge requires; herald's own queue alone, refused with `runs = "forge"` | none |
| `wait` | how long herald waits for a head's checks before the pull request leaves; herald's own queue alone, refused with `runs = "forge"` | `"2h"` |
| `retries` | `{ checks = [...], times = N }`: re-run the failed job of a check named, at most N times on one head, before the pull request leaves; not on Forgejo; herald's own queue alone, refused with `runs = "forge"` | none |
| `role_lost` | what `--apply` does with a pull request herald handed to the forge's queue whose gesture's maker no longer holds `least`: `"take-out"` takes it out, `"stay"` leaves it there and says so; `runs = "forge"` alone | `"take-out"` |
| `applied_by` | `"command"`: `herald merge-queue --apply` takes the steps; `"server"`: `herald server` takes them each round and as webhooks arrive, and then reads the write credential | `"command"` |
| `write` | the credential the steps are made with: `{ token = { secret } }` or `{ app = "<github_apps name>" }` | none: the queue does not start |

A source sets how herald brings a branch up to date:

| Key | What it is | Default |
|-----|------------|---------|
| `sources.<name>.update` | `"merge"`: merge the base into the branch through the forge, keeping the author's commits and signatures; `"rebase"`: rebase it through the forge. GitLab offers a rebase alone | `"merge"` on GitHub and Forgejo, `"rebase"` on GitLab |

## Queueing a pull request

A writer adds the label, or writes the comment. herald reads who did it,
and their role on the repository, when it takes the pull request in and
again right before each step it takes. Forgejo and Gitea tell a
person's role on a repository to its administrators alone, so there the
source's read token is a repository administrator's. The gesture
vouches for one head, as `vouches` says: a push past it takes the pull
request out, and it comes back only by a new gesture. Where several
gestures were made, the last one counted stands, so a new comment
command after a push vouches again. An edited comment is never a
gesture; an edited comment command followed by `cancel` still takes the
pull request out. Removing the label, by anyone and also by an account
since deleted, deleting the comment, or the comment command followed by
the word `cancel` from the author, a writer or the person who queued it takes it
out. Pull requests go first in, first out per branch; nothing jumps the
queue.

### Which head a gesture vouches for

| `vouches` | The head it vouches for | What it accepts |
|-----------|-------------------------|-----------------|
| `"gesture"` | the head the pull request had at the gesture's time on the forge, by the forge's own record of pushes to its branch (GitHub's repository activity, GitLab's diff versions, Forgejo's timeline), a push stamped up to `skew` before the gesture counted as after it | nothing past that record: where it cannot be read, as for a fork whose activity the read credential does not reach, a label vouches for no head. GitHub's activity, read newest first, may drop old pushes, so there a label places a head only after a push the activity lists, and otherwise vouches for none. A GitLab merge request with 100 diff versions or more, or a Forgejo timeline longer than herald reads, gives no record |
| `"seen"` | the head herald first reads with the gesture standing, by the store's record of it | the window between the gesture and herald's first read: a push in it, by the author, a fork's author too, is vouched for as if the writer had seen it; under `"approved"` only where a writer approved that head. This is why `"gesture"` is the default. `herald merge-queue --apply` writes that record the first time it reads the gesture standing; the dry run only reads it, and where none is kept takes the head it reads now as the one `--apply` would record. Where the record cannot be read or written, the pull request leaves as `unrecorded` |
| `"approved"` | the head herald first reads with the gesture standing, by the store's record of it, once a person holding `least` has approved that exact commit on the forge | the forge's review record. On GitHub and Forgejo each reviewer's last review that approves or asks for changes counts, and approves only where it names that commit; a dismissed one counts against. A GitLab approval names no commit: it counts for the head of the latest diff version only where GitLab's own note of the approval is stamped more than `skew` after that version, and a note that the approval was taken back undoes it. The record of the first-seen head is kept as for `"seen"` |

A comment command followed by a commit's full id (`/merge 3f2a…`)
vouches for that commit alone, whatever `vouches` says, so it works
where the forge's record of pushes cannot be read. It stands in for the
record of the head and never for the approval `"approved"` asks. Each read of the
gestures, pushes or reviews that comes back partial leaves the pull
request as `unread`.

## The forge's queue

With `runs = "forge"` herald checks the gesture, that the pull request is
open and not a draft and that its head is the one vouched for, then hands
it over: GitHub's `enqueuePullRequest` with the expected head, or
GitLab's merge train with the head's `sha`. The forge proves and merges
it.

Right before the call, `herald merge-queue --apply` reads the pull
request again on its own, with nothing kept from the queue's read: that
it is open into the branch, not a draft, still carries the label where
the gesture is one, that the gesture standing is the same one (the
forge's id of the label event or comment, who made it and when), that
its maker holds `least` now, that its head is the one handed over and
the one the gesture vouches for, and that every one of those reads was
whole. Any of them failing, or any read failing, hands nothing over.

| Forge | The call | What the forge checks itself |
|-------|----------|------------------------------|
| GitHub, GHE.com, GitHub Enterprise Server | GraphQL `enqueuePullRequest(input: { pullRequestId, expectedHeadOid })` at the GraphQL endpoint beside the source's API (`/graphql`, or `/api/graphql` on a server), the pull request named by its node id | that the credential may write to the repository, that the pull request's head is `expectedHeadOid`, and the branch's protection; the merge queue then proves it on the base. GitHub answers a refusal as a GraphQL error |
| GitLab.com, Self-Managed, Dedicated (Premium, Ultimate) | `POST /projects/:id/merge_trains/merge_requests/:iid` with `sha` and `auto_merge = true`: 201 when it is added, 202 when it joins once its pipeline succeeds | that the credential holds the Developer role or above, and that `sha` is the source branch's head, or the merge fails |

Both forges take the head herald hands over as the head they must find,
so a head pushed after herald's read is never merged. GitHub refuses the
call; herald takes a refusal whose pull request reads back with another
head as `head-moved`. GitLab documents no status for a `sha` that is not
the head: it says the merge fails. GitLab does not document what adding
a merge request does on a project whose merge trains are off, so herald
hands a merge request over only where GitLab says the project has them
on. A refusal is shown with the forge's own words, without the
credential. GitHub documents no permission an App needs for
`enqueuePullRequest`, and GitLab no error status for adding to a train:
herald mints `contents: write` and `pull_requests: write`, and takes any
4xx answer but 408 and 429 as a refusal. An answer that is lost, or
says neither, is read back (GitHub's `isInMergeQueue`, GitLab's train
car or the merge request's `merge_when_pipeline_succeeds`) and never
asked again. `queue = true` on a view shows where it stands (see [the
queue](queue.md#merge-queue)). Where the forge has no queue on the
branch, the queue does not start and `herald doctor` says why (the
forge, its tier or the kind of repository) and that `runs = "herald"`
runs herald's own. On GitHub a branch under
classic branch protection may have a merge queue the REST API does not
show; there doctor says the forge would not say, and each pull request
is handed over all the same.

## herald's own queue

With `runs = "herald"` one pull request per branch is in flight. herald:

1. brings it up to date with the base, unless its head already holds the
   base's head;
2. waits for its checks on the new head;
3. merges it, giving the head it proved as the expected head, so a head
   that moved is never merged.

A base that moves before the merge sends it back to step 1. It is green
only where every check on the head passed, every check the forge
requires on the base is there and passed, and the forge says it can
merge, which holds the reviews it requires. A head on which no check
that counts is reported is waited for, and leaves as `checks-timed-out`
once `wait` has passed; it is never merged at once. A check `may_fail`
names never counts for this. herald never bypasses a
protection. On a GitHub branch whose rules allow merging through the
merge queue alone, the forge refuses herald's own merge, and `herald
doctor` says so first.

A pull request leaves at once, with its reason, when a check fails
(`checks-failed`), its checks outlast `wait` (`checks-timed-out`), its
update conflicts (`conflict`) or cannot be made (`cannot-update`, as for
a fork that does not let maintainers edit), its head moves
(`head-moved`), herald holds no record of the head its gesture vouches
for (`unrecorded`), its gesture is taken back (`removed`), the forge says it
cannot merge (`unmergeable`) or refuses the merge (`merge-refused`), or
what decides whether it stands or is green could not be read whole
(`unread`): a failure, never taken as `removed`. herald reads at most 500
of a pull request's label events, comments or timeline entries and one
page of its head's checks, so a pull request with more leaves as
`unread`. A queue whose branch has more than 500 open pull requests, or
more than 50 that may hold a gesture (those carrying the label, or every
open one where the gesture is a comment), is not read at all: `herald
merge-queue` says why, and herald takes no step in it.
A view's row holds the reason in `queue.left`, and a rule with `dequeued
= true` tells people.

## Running the steps

```sh
herald merge-queue                 # each queue's entries, verdicts and next steps; reads no write credential
herald merge-queue app-main --apply   # take the next steps
```

Without `--apply` nothing is written and no write credential is read:
herald prints what `--apply` would do, reading the store's records as
`--apply` does, and a step recorded for a gesture with what came of it.
A queue that does not start, whose steps `herald server` takes
(`applied_by = "server"`), that herald runs (`runs = "herald"`: `--apply`
hands pull requests to the forge's queue alone), or whose forge says the
branch has no merge queue, takes no step, and says why.

### Taking a pull request back out

With `runs = "forge"`, `herald merge-queue` also reads the forge's queue
on the branch: on GitHub the merge queue's entries, on GitLab the merge
requests on the active merge train and the open ones set to join it once
their pipeline succeeds. Past 50 pull requests it reads none and says
why.

A take-out is judged against herald's record of what it handed over,
never against which marks happen to count now. Its anchor is the newest
gesture on the pull request whose hand-off herald recorded as reached:
queued, being called, unsettled, or found already in the forge's queue.
A pull request with no such gesture, as one queued on the forge by hand,
stays and is not listed. Against the anchor, in this order:

1. A gesture made after the anchor, and after any cancel of rule 2, by
   one who holds `least` now stands: nothing is taken out. It is a new
   gesture; the next run records it, found already queued.
2. The newest cancel made after the anchor (the label removed, the
   comment deleted, or the command followed by `cancel`) by one who
   holds `least` now takes it out, where the forge names an id for it.
   It is carried out whatever `role_lost` says.
3. Where the anchor's maker no longer holds `least` (a lower role, none,
   or an account since deleted, which the forge names as someone else
   and herald reads as holding no role), `role_lost = "take-out"`, the
   default, takes it out, said as the role lost; `role_lost = "stay"`
   leaves it there and says so. A role the forge would not say is not a
   lost one: nothing is taken out, and herald says why.

Marks made before the anchor, and marks by makers below `least`, never
count for or against. Where the anchor's maker holds `least` and no
cancel of rule 2 stands, the newest cancel after the anchor (by one
below `least`, or one the forge named no id for) is shown as one herald
could not carry out, and the pull request stays; one who holds `least`
may take it back again. A cancel stamped up to `skew` before a gesture
counts as made after it. Each pull request taken out, or shown, is
listed with the cancel judged, who made it, their role now, and what
`--apply` does with it. Marks read in part take nothing out. Right
before the call herald reads the pull request again on its own, finds
the gesture it handed over by its id and time, judges it the same way,
and calls only where the same cancel, or the same lost role, takes it
out.

| Forge | The call |
|-------|----------|
| GitHub, GHE.com, GitHub Enterprise Server | GraphQL `dequeuePullRequest(input: { id })`, the pull request named by its node id |
| GitLab.com, Self-Managed, Dedicated | `POST /projects/:id/merge_requests/:iid/cancel_merge_when_pipeline_succeeds`, on every GitLab version: it takes the merge request off the merge train, or stops it joining once its pipeline succeeds |

The take-out is kept in the gesture's record as the hand-off is: written
with compare-and-set before the call, keyed by the cancel's id, who made
it and when, and after the call what came of it. A lost answer, or a
refusal, is read back (GitHub's `isInMergeQueue`, GitLab's train car or
`merge_when_pipeline_succeeds`); a pull request found out counts as
taken out. A take-out recorded as being made, refused or lost is never
asked again for that cancel; one that provably never left herald is.
A queue that takes no step holds its take-outs back too.

Run `--apply` on a schedule or in a CI job of the base repository's own,
never in a job that runs a pull request's code. `herald merge-queue`
prints a private repository's entries only where its output is said
private, as `herald queue` does.

## The write credential

It is held apart as a re-run's is (see [applying a
re-run](queue.md#applying-a-re-run)): the config does not load where its
secret is one herald reads with, and it goes to the source's API alone,
following no redirect. It may be the same App or secret as the source's
`write`. Give it no more than the steps need:

| Forge | Credential |
|-------|------------|
| GitHub | an App installed on the repositories concerned with `contents: write` and `pull_requests: write` (and `actions: write` for `retries`); herald mints each step's token for the one repository with that step's permissions alone, and revokes unused a token GitHub says holds more. Or a fine-grained token with contents and pull requests write on those repositories alone |
| GitLab | a project access token with the `api` scope and the role the branch's protection lets merge |
| Forgejo, Gitea | a token with the `write:repository` scope, of a user who may merge into the branch |

herald keeps one record per gesture, keyed by the forge's API, the
repository, the pull request's number and the forge's own id of the
label event or comment. The record also names who made the gesture and
when, and both are compared on every read: where the forge names
another maker or time for that id, it is not the same gesture, and
herald writes nothing for it and says so; taking out compares the time
alone, so an account since deleted still has its pull request taken out
for the role lost. A gesture made again, even in the same second, is
another record, and a gesture that does not count (its maker below
`least`) gets none. `--apply` alone writes it: the head it first read
with the gesture standing, then, before the call and with
compare-and-set, the hand-off (the head handed over, the base's head,
the credential's name, the time), and after the call what came of it.
Right before writing the hand-off, herald reads on its own whether the
pull request is already in the forge's queue. Where it is, herald makes
no call and records the hand-off, with compare-and-set, as found
already queued: no later run hands it over for that gesture, and a
cancel or a lost role takes it out as if herald had handed it over.
Where that read fails or is not whole, nothing is handed over and
nothing written. So two runs racing never both act, and a gesture is
handed over at most once: a record that says the call was being made, that the forge's
answer was lost, that the forge refused it or that its head moved is
never called again; a new gesture is. Only a record whose call
provably never left herald (the run's request budget ended, the write
App's token could not be minted, or the request could not be made) is
handed over again, after a fresh read. A gesture the forge names no id
for is never handed over. A store that keeps no records (`markers`) runs
no queue. The forge's audit log names the credential; herald's records
name who queued.

## Hosts

Every read and step goes to the source's API: github.com, a GHE.com
tenant, GitHub Enterprise Server, GitLab.com, Self-Managed and
Dedicated, and any Forgejo or Gitea host. A GitHub Enterprise Server
whose GraphQL lacks `enqueuePullRequest` takes `runs = "herald"` alone.

## Not built

Batches and speculative trains (several pull requests proved together on
a temporary branch), priorities and jumping the queue.
