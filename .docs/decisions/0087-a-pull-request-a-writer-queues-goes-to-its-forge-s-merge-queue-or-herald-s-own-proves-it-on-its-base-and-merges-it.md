# ADR-0087: a pull request a writer queues goes to its forge's merge queue, or herald's own proves it on its base and merges it

**Status:** Accepted
**Date:** 2026-11-24

## Context

herald reads where a pull request stands in its forge's merge queue
(ADR-0078) and re-runs one planned job at an operator's word (ADR-0068).
It merges nothing. Teams on a forge without a merge queue (Forgejo,
Gitea, GitHub personal repositories and private organization
repositories outside GitHub Enterprise Cloud, GitLab without merge
trains) merge by hand, and a branch protected by "require up to date"
makes each author race the others.

What the forges offer (read 2026-11-24):

| Forge | Native queue | Merge call | Bringing a branch up to date |
|-------|--------------|------------|------------------------------|
| GitHub, GHE.com, GitHub Enterprise Server | merge queue on organization repositories (private ones on GHE Cloud and Server alone): GraphQL `enqueuePullRequest(expectedHeadOid)`, `dequeuePullRequest` | `PUT …/pulls/{n}/merge` with `sha` and `merge_method` (405 cannot merge, 409 head moved) | `PUT …/pulls/{n}/update-branch` with `expected_head_sha`: merges the base in |
| GitLab.com, Self-Managed, Dedicated | merge trains on Premium and Ultimate: `POST …/merge_trains/merge_requests/:iid` with `sha` and `auto_merge` | `PUT …/merge_requests/:iid/merge` with `sha` (405, 409 sha mismatch, 422) | `PUT …/merge_requests/:iid/rebase`, the only call |
| Forgejo, Gitea | none | `POST …/pulls/{index}/merge` with `Do` and `head_commit_id` (405, 409, 423) | `POST …/pulls/{index}/update?style=merge\|rebase` |

The user decided, on 2026-11-24: the forge's own queue by default and
herald's own as a setting per branch; a writer's label or comment puts a
pull request in, checked at each step; herald's queue is serial and
proves a pull request on its base; the base is merged in by default; a
command of its own holds the write credential by default; green means
every check, the forge's required ones and the forge's mergeable;
the merge method is named, never guessed; a failure leaves the queue at
once.

## Decision

1. **A merge queue is a table per base branch.** `[merge_queues.<name>]`
   names a `source` of kind `github`, `gitlab` or `forgejo`, the
   `repository` where the source is an owner, and one `branch`. Two
   tables naming the same repository and branch do not load.
2. **The forge's queue by default, herald's own as a setting.** `runs =
   "forge"` (the default) hands each pull request to the forge's merge
   queue or merge train; herald proves nothing and merges nothing
   itself. `runs = "herald"` makes herald's own serial queue run the
   branch, also where the forge has a queue of its own. On a branch
   whose forge rules let a pull request merge only through the forge's
   queue, herald's own merge is refused by the forge, and doctor says so
   before it is tried. Where `runs = "forge"` and the forge has no queue
   on the branch, the queue refuses to start and doctor names why (the
   forge, the tier or the repository kind) and that `runs = "herald"`
   runs herald's own.
3. **A writer's gesture puts a pull request in.** `enter = { label =
   "<name>" }`, `"herald:merge"` where absent, or `enter = { comment =
   "<command>" }`: a comment whose whole body, trimmed, is the command,
   or the command followed by a commit's full id.
   `least` names the least role the person who made the gesture holds on
   the repository: `"write"` (GitHub write, GitLab Developer, Forgejo
   write, or above; the default), `"maintain"` or `"admin"`. herald reads
   who made the gesture from the forge (GitHub's issue events, GitLab's
   resource label events or notes, Forgejo's timeline) and their role
   (GitHub's collaborator permission, GitLab's project member access
   level, Forgejo's collaborator permission), first when it takes the
   pull request in and again right before each step. A label removed, a
   comment deleted, or a person who no longer holds the role takes the
   pull request out. The pull request's author, a writer or the person
   who made the gesture takes it out with the comment command followed by
   the word `cancel`. A pull request enters again only by a gesture made after it
   left. Of several gestures, the last one counted stands. An edited
   comment is never a gesture; an edited comment command followed by
   `cancel` still takes the pull request out. A label removed by an
   account since deleted still takes it out.
4. **A gesture vouches for one head.** `vouches` says which, and a head
   that changes past it by any push herald did not make takes the pull
   request out (`head-moved`). `"gesture"` (the default): the head the
   pull request had at the gesture's forge time, by the forge's own
   record of pushes to its branch (GitHub's repository activity, GitLab's
   merge request diff versions, Forgejo's timeline); where that record
   cannot be read whole, a label vouches for no head; GitHub's activity,
   read newest first, may drop old pushes, so there a label places a head
   only after a push it lists; a GitLab merge request with 100 diff
   versions or more, or a Forgejo timeline longer than herald reads,
   gives no record. `skew` ("5s" where absent) is how far apart the forge's
   stamps of a gesture and of a push may be, its services' clocks never
   quite agreeing: a push stamped up to `skew` before the gesture counts
   as after it, so the gesture never vouches for a head that push may
   have brought. `"seen"`: the head herald first reads with the gesture
   standing, by the store's record of it, accepting the window between
   the gesture and that first read, in which a push is vouched for as if
   the writer had seen it. `"approved"`: that first-seen head once a
   person holding `least` has approved that exact commit on the forge,
   accepting the forge's review record as the vouch: on GitHub and
   Forgejo each reviewer's last review that approves or asks for changes,
   which approves only where its `commit_id` is that commit, a dismissed
   one counting against; GitLab's approvals name no commit, so one
   counts for the head of the latest diff version only where GitLab's own
   note of it is stamped more than `skew` after that version, and a note
   that it was taken back undoes it. Without the store's record of the
   first-seen head, `"seen"` and `"approved"` vouch for none and the pull
   request leaves as `unrecorded`. A comment command followed by a
   commit's full id vouches for that commit alone under every value: it
   stands in for the record of the head, never for the approval.
   Every read of the gestures, pushes or reviews that comes back partial
   leaves the pull request as `unread`.
5. **Order.** First in, first out per base branch, by the forge's time
   of each gesture. Nothing jumps the queue.
6. **The hand-off.** With `runs = "forge"` herald checks the gesture,
   that the pull request is open and not a draft, and that its head is
   the head vouched for; then it calls GitHub's `enqueuePullRequest` with
   `expectedHeadOid`, or adds the merge request to GitLab's train with
   `sha` and `auto_merge = true`. The forge's queue decides the rest; its
   entries and reasons for leaving are read as ADR-0078 reads them. A
   gesture taken back takes the pull request out of the forge's queue
   (`dequeuePullRequest`; on GitLab, the train's auto-merge is
   cancelled).
7. **herald's own queue is serial.** With `runs = "herald"` one pull
   request per base branch is in flight. herald brings it up to date with
   the base, waits for its checks on the new head, and merges it with the
   head it proved as the expected head (`sha`, `head_commit_id`). A base
   that moves before the merge brings it up to date again. A pull request
   whose head already holds the base's head is not updated.
8. **Up to date by merging the base in.** `[sources.<name>] update =
   "merge"` (the default on GitHub and Forgejo) merges the base into the
   branch through the forge, which keeps the author's commits and their
   signatures; `"rebase"` rebases it through the forge. GitLab offers a
   rebase alone: its default is `"rebase"`, and `"merge"` does not load
   there. A fork whose author does not let maintainers edit cannot be
   updated: it leaves the queue (`cannot-update`), the author's turn.
9. **Green.** By default herald merges only where every check reported
   on the head has passed (GitHub's check runs and statuses, GitLab's
   commit statuses, Forgejo's statuses), every check the forge requires
   on the base is present and passed (GitHub's rules for the branch,
   GitLab's and Forgejo's protection where readable), and the forge says
   the pull request is mergeable (GitHub's merge state `clean`, GitLab's
   detailed merge status `mergeable`, Forgejo's `mergeable`), which holds
   the reviews and approvals the forge requires. `may_fail` names checks
   that may be red or missing on that branch; a check the forge requires
   stays required whatever `may_fail` says. A head on which no check
   that counts is reported, a check `may_fail` names never counting, is
   waited for and leaves as `checks-timed-out`, never merged at once. A check still running is
   waited for until `wait` ("2h" where absent) has passed since the head
   was proved up to date. herald never bypasses a protection: no
   `bypass_rules`, no `force_merge`, no admin merge.
10. **The method is named.** `method` is `"merge"`, `"squash"`,
    `"rebase"`, `"fast-forward"` (Forgejo's fast-forward-only, GitLab's
    fast-forward project setting;
    GitHub has none) or `"repository"` (the repository's
    single allowed method, as the forge names it; refused where it allows
    more than one). A queue without `method` refuses to start. The commit
    message is the forge's own for that method; `message = "pull"` makes
    it the pull request's title and body, with `"merge"` or `"squash"`
    alone. herald refuses a method the repository does not allow, and on
    GitLab one other than the project's merge method (squash beside it
    as the project allows). With `runs = "forge"` the method is the forge
    queue's own, and a `method` naming another does not start.
11. **A failure leaves at once.** A pull request whose check fails, whose
    checks time out, whose update conflicts or fails, whose head moves,
    whose gesture is taken back, that the forge says cannot merge, whose
    merge the forge refuses, or whose record herald could not read whole,
    leaves herald's queue at once. Its `Pull.queue.left` holds when and
    why (`checks-failed`, `checks-timed-out`, `conflict`,
    `cannot-update`, `head-moved`, `unrecorded`, `removed`,
    `unmergeable`, `merge-refused`, `unread`), so ADR-0078's `dequeued` rule tells
    people, every reason but `removed` a failure of the pull request.
    `unread` says that what decides whether a pull request stands or is
    green could not be read whole; it counts as a failure and is never
    taken as `removed`. `retries = { checks = [...], times = N }` re-runs
    the failed job of a check it names, at most N times on one head,
    through ADR-0068's narrowest call, before the pull request leaves;
    none where absent; Forgejo has no re-run call, so its queues take no
    `retries`. `may_fail`, `wait` and `retries` shape herald's own queue
    alone, so a queue with `runs = "forge"` that gives one does not load.
12. **A command of its own holds the write credential.** `herald
    merge-queue [queue]` prints each queue's entries with the verdict and
    the step that comes next, reading no write credential. `--apply`
    takes those steps, and is meant to run on a schedule or a CI job of
    the base repository's own. `applied_by = "server"` lets `herald
    server` take the steps each round and as webhooks wake it; the server
    then reads that queue's write credential, and doctor says so.
    `"command"` is the default.
13. **The write credential.** `[merge_queues.<name>] write` is a token
    (`{ token = { secret } }`) or a GitHub App (`{ app }`), held apart
    from every credential herald reads with by ADR-0068's rules, and sent
    only to the source's API origin through a connection that follows no
    redirect. It may name the same App or secret as its source's
    `write`. A write App's token is minted for one step and one
    repository with that step's permissions alone, and GitHub's answer
    must name those permissions (beside `metadata: read`) on that
    repository and nothing more, or it is revoked unused: `contents:
    write` to merge, `contents: write` and `pull_requests: write` to bring
    a branch up to date or hand a pull request to the merge queue,
    `actions: write` to re-run a job. A GitHub token is best a
    fine-grained one holding contents and pull requests write on the
    repositories concerned alone; a GitLab token a project access token
    with the `api` scope and the role the branch's protection lets merge;
    a Forgejo token one with the `write:repository` scope of a user who
    may merge into the branch.
14. **The record comes before each step.** The `Actions` store port keeps
    one record per queue, pull request, head and step (the gesture's
    person, the credential's name, the base's head, time, outcome),
    written as new with compare-and-set before the call, as ADR-0068's
    re-run record is: of two runs racing only the first calls, and a
    crash after the record leaves a step no run takes again. A lost
    answer is never asked again: herald reads the pull request back and
    records what came of it. The markers store keeps no records, so a
    queue on it does not start.
15. **The setup wizard sets every key.** `herald setup merge-queue`
    asks for each key in turn, with its default chosen and the forge's
    own answers shown (whether the branch has a queue, the methods the
    repository allows), offers `runs = "herald"` where the forge has no
    queue on the branch, and writes the `[merge_queues.<name>]` table
    into the config once the operator confirms it.
16. **Doctor names, for each queue,** the forge and whether it has a
    queue on the branch, the method and whether the repository allows
    it, which head a gesture vouches for and its `skew`, the update style, whether the write credential is set and held
    apart, whether the store keeps records, and where the steps run.
17. **Every forge's own host.** Each read and step goes to the source's
    configured API base: github.com, a GHE.com tenant, GitHub Enterprise
    Server (whose merge queue serves organization repositories; a server
    whose GraphQL lacks `enqueuePullRequest` takes no `runs = "forge"`,
    and herald never calls `merge-async`), GitLab.com, Self-Managed and
    Dedicated (merge trains on Premium and Ultimate), and any Forgejo or
    Gitea host (herald's own queue alone).
18. **What is said and kept.** `herald merge-queue` prints each step and
    its outcome, and a private repository's entries only where its output
    is said private (ADR-0069). The records of point 14 are the log: no
    record, line or answer holds a credential or a minted token.

## Who can cause a merge

| Who | What they can do |
|-----|------------------|
| A person who may only label (GitHub's triage role) | Nothing: the label's maker must hold `least`. Removing a label takes a pull request out, the safe direction. |
| A writer | Queue a pull request at the head they saw. They could merge it by hand. |
| The author without write, after a writer queued it | With `vouches = "gesture"`, a push after the gesture takes the pull request out (`head-moved`); it re-enters only by a new gesture. With `"seen"`, a push before herald's first read of the gesture is vouched for; with `"approved"`, only where a writer approved that commit. |
| A forge record herald cannot read whole (pushes, gestures, reviews) | Nothing stands on it: the pull request leaves as `unread`, and a label vouches for no head. |
| A comment from someone without the role | Ignored; an edited comment counts as its author's. |
| A fork pull request | Its CI and checks are the forge's; herald runs none of its code and checks out nothing. A fork that does not let maintainers edit is never updated. |
| A forged or replayed webhook to `herald server` | It only wakes the queue: every step reads the gesture, the role, the head, the checks and mergeable from the forge's API first. |
| A network position between herald and the forge | TLS to the source's API origin alone, no redirect followed; a lost answer is read back, never asked again. |
| A compromised `herald server` with `applied_by = "command"` | It holds no write credential, so it merges nothing. |
| A stolen write credential | It merges without herald: give it no more than the steps need, on the repositories concerned. |

## Consequences

- The forge's audit log names the write credential, not the person who
  queued; herald's records name them.
- A command-run queue moves as often as its schedule runs it; a
  server-run one at each webhook.
- Batches and speculative trains (several pull requests proved together
  on a temporary branch) are not built: herald's own queue proves one
  pull request at a time. Priority and jumping the queue are not built.
- Entries of herald's own queue ride their rows as ADR-0078's do, so a
  private repository's queue is shown, withheld and counted as its rows
  are.
