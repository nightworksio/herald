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

`herald setup merge-queue` asks for each key in turn, with its default
chosen and what the forge says shown beside it (whether the branch has a
queue, which methods the repository allows), offers `runs = "herald"`
where the forge has no queue on the branch, and writes the table into
the config once you confirm it.

| Key | What it is | Default |
|-----|------------|---------|
| `source` | a source of kind `github`, `gitlab` or `forgejo` | required |
| `repository` | the repository, where the source is an owner | the source's own |
| `branch` | the base branch the queue merges into; one table per branch | required |
| `runs` | `"forge"`: hand each pull request to the forge's merge queue or merge train; `"herald"`: herald's own serial queue, also where the forge has one | `"forge"` |
| `enter` | `{ label = "<name>" }`, or `{ comment = "<command>" }`: a comment whose whole body is the command, or the command and a commit's full id | `{ label = "herald:merge" }` |
| `least` | the least role the person who queues holds: `"write"` (GitHub write, GitLab Developer, Forgejo write, or above), `"maintain"` or `"admin"` | `"write"` |
| `vouches` | which head a gesture vouches for: `"gesture"`, `"seen"` or `"approved"` (see [which head a gesture vouches for](#which-head-a-gesture-vouches-for)) | `"gesture"` |
| `method` | `"merge"`, `"squash"`, `"rebase"`, `"fast-forward"`, or `"repository"`: the repository's one allowed method | required: a queue without it does not start |
| `message` | `"forge"`: the forge's own commit message for the method; `"pull"`: the pull request's title and body, with `"merge"` or `"squash"` | `"forge"` |
| `may_fail` | checks that may be red or missing on this branch; never one the forge requires; herald's own queue alone, refused with `runs = "forge"` | none |
| `wait` | how long herald waits for a head's checks before the pull request leaves; herald's own queue alone, refused with `runs = "forge"` | `"2h"` |
| `retries` | `{ checks = [...], times = N }`: re-run the failed job of a check named, at most N times on one head, before the pull request leaves; not on Forgejo; herald's own queue alone, refused with `runs = "forge"` | none |
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
request out, and it comes back only by a new gesture. Removing the
label, deleting the comment, or the comment command followed by
the word `cancel` from the author, a writer or the person who queued it takes it
out. Pull requests go first in, first out per branch; nothing jumps the
queue.

### Which head a gesture vouches for

| `vouches` | The head it vouches for | What it accepts |
|-----------|-------------------------|-----------------|
| `"gesture"` | the head the pull request had at the gesture's time on the forge, by the forge's own record of pushes to its branch (GitHub's repository activity, GitLab's diff versions, Forgejo's timeline) | nothing past that record: where it cannot be read, as for a fork whose activity the read credential does not reach, a label vouches for no head |
| `"seen"` | the head herald first reads with the gesture standing, kept in the store's record | the window between the gesture and herald's first read: a push in it is vouched for as if the writer had seen it |
| `"approved"` | the head herald first reads with the gesture standing, once a person holding `least` has approved that exact commit on the forge | the forge's review record: GitHub's and Forgejo's reviews name their commit; a GitLab approval counts only where the project removes approvals when a commit is added |

A comment command followed by a commit's full id (`/merge 3f2a…`)
vouches for that commit alone, whatever `vouches` says, so it works
where the forge's record of pushes cannot be read. Each read of the
gestures, pushes or reviews that comes back partial leaves the pull
request as `unread`.

## The forge's queue

With `runs = "forge"` herald checks the gesture, that the pull request is
open and not a draft and that its head is the one vouched for, then hands
it over: GitHub's `enqueuePullRequest` with the expected head, or
GitLab's merge train with the head's `sha`. The forge proves and merges
it. `queue = true` on a view shows where it stands (see [the
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
merge, which holds the reviews it requires. herald never bypasses a
protection. On a GitHub branch whose rules allow merging through the
merge queue alone, the forge refuses herald's own merge, and `herald
doctor` says so first.

A pull request leaves at once, with its reason, when a check fails
(`checks-failed`), its checks outlast `wait` (`checks-timed-out`), its
update conflicts (`conflict`) or cannot be made (`cannot-update`, as for
a fork that does not let maintainers edit), its head moves
(`head-moved`), its gesture is taken back (`removed`), the forge says it
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

Before each step herald writes its record (the queue, pull request,
head, base, who queued it, the credential's name, the time), so two runs
racing never both act and a step interrupted is never taken twice; a
store that keeps no records (`markers`) runs no queue. The forge's audit
log names the credential; herald's records name who queued.

## Hosts

Every read and step goes to the source's API: github.com, a GHE.com
tenant, GitHub Enterprise Server, GitLab.com, Self-Managed and
Dedicated, and any Forgejo or Gitea host. A GitHub Enterprise Server
whose GraphQL lacks `enqueuePullRequest` takes `runs = "herald"` alone.

## Not built

Batches and speculative trains (several pull requests proved together on
a temporary branch), priorities and jumping the queue.
