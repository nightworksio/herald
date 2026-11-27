# ADR-0087: a pull request a writer queues goes to its forge's merge queue, or herald's own proves it on its base and merges it

**Status:** Accepted; decision 3's gestures extended with the chat gesture by [ADR-0098](0098-a-chat-user-acts-on-the-forge-only-as-the-person-the-operator-names-signed-in-at-the-forge-through-an-offer-bound-to-its-post.md)
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

A GitHub ruleset's `merge_queue` rule says merges must be made through
the merge queue, and names its `merge_method`; GitHub's merge queue
serves an organization's public repositories, its private ones on
GitHub Enterprise Cloud, and every organization repository on GitHub
Enterprise Server. GitLab 19.3 and later can enforce a project's merge
train (`merge_train_enforcement` at `enforce_for_all_users` or
`enforce_with_owner_override`); the merge API then refuses a direct merge
with 405. By default (`allow_bypass`) GitLab merges a merge request
directly beside its train (read 2026-11-27).

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
   queue (a GitHub ruleset's merge queue rule, GitLab's merge train
   enforcement), herald's own merge is refused by the forge, and doctor
   says so before it is tried. Where `runs = "forge"` and the forge has
   no queue on the branch, or none herald can hand a pull request to, the
   queue refuses to start and doctor names why (the forge; a personal
   GitHub repository; a private one on github.com outside GitHub
   Enterprise Cloud; no ruleset requiring a merge queue; GitLab's merge
   trains off, a Premium and Ultimate feature; a GitHub Enterprise Server
   without `enqueuePullRequest`) and that `runs = "herald"` runs herald's
   own.
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
   that it was taken back undoes it. The store's record of the
   first-seen head is written by `herald merge-queue --apply` alone, the
   first time it reads the gesture standing; without it, `"seen"` and
   `"approved"` vouch for none and the pull request leaves as
   `unrecorded`. A comment command followed by a
   commit's full id vouches for that commit alone under every value: it
   stands in for the record of the head, never for the approval.
   Every read of the gestures, pushes or reviews that comes back partial
   leaves the pull request as `unread`.
5. **Order.** First in, first out per base branch, by the forge's time
   of each gesture. Nothing jumps the queue.
6. **The hand-off.** With `runs = "forge"` herald checks the gesture,
   that the pull request is open and not a draft, and that its head is
   the head vouched for, each read again on its own right before the
   call with nothing kept from the queue's read; then it calls GitHub's
   `enqueuePullRequest` with `expectedHeadOid`, or adds the merge request
   to GitLab's train with `sha` and `auto_merge = true`, so the forge
   itself refuses a head that moved after that read. Right before the
   call herald also reads whether the pull request is already in the
   forge's queue: where it is, no call is made and the hand-off is
   recorded as found already queued, which a cancel or a lost role takes
   out as one herald handed over; where that read fails, nothing is
   handed over or written. The forge's queue decides the rest; its
   entries and reasons for leaving are read as ADR-0078 reads them. A
   pull request herald handed over is taken out of the forge's queue
   again, with GitHub's `dequeuePullRequest`, or GitLab's `POST
   /projects/:id/merge_requests/:iid/cancel_merge_when_pipeline_succeeds`
   on every GitLab version, which takes it off the train or stops it
   joining. The take-out is anchored on herald's record of what it
   handed over, never on which marks count now: the anchor is the newest
   gesture on the pull request with a reached hand-off record (queued,
   calling, unsettled or found already queued); a pull request queued on
   the forge by hand has none and stays. Against the anchor: a gesture
   made after it, and after any cancel that counts, by one who holds
   `least` now stands, and nothing is taken out; else the newest cancel
   made after it by one who holds `least` now, that the forge names an
   id for, takes it out; else, where the anchor's maker no longer holds
   `least` (a lower role, none, or an account since deleted), it is
   taken out with `role_lost = "take-out"`, the default, or left with
   `role_lost = "stay"`, said either way. Marks made before the anchor,
   and marks by makers below `least`, never count for or against; such
   a cancel after the anchor is shown as one herald could not carry out
   where the anchor's maker holds `least`. A cancel stamped up to `skew`
   before a gesture counts as made after it. A role the forge would not
   say is not a lost one: nothing is taken out, and herald says why.
   Right before the call the pull request is read again on its own and
   judged the same way; only the same cancel, or the same lost role,
   takes it out.
7. **herald's own queue is serial.** With `runs = "herald"` one pull
   request per base branch is in flight. herald brings it up to date with
   the base, waits for its checks on the new head, and merges it with the
   head it proved as the expected head (`sha`, `head_commit_id`). A base
   that moves before the merge brings it up to date again. A pull request
   whose head already holds the base's head is not updated. Right before each
   step herald reads the pull request, its base, its checks and whether
   the forge can merge it again on their own, and takes the step only
   where every input was read whole and it is still due for that step at
   that head. Where the gesture no longer stands, herald judges every
   mark on the pull request against it: a newer gesture or a later cancel
   by one holding `least` now takes no step, marks by anyone else count
   for nothing, and a gesturer below `least` now or with no role takes
   it out (`removed`), recorded; a role the forge would not say takes no
   step.
   GitLab's rebase and Forgejo's update take no head to find, so herald
   reads the head again right before that call. The head herald's own
   update makes vouches as `updated` says, which defaults to `"signed"`
   where herald can prove the forge's signature (on github.com, and on
   GitHub Enterprise Server and GHE.com where `signer` is set) and to
   `"never"` elsewhere, a value given always standing: `"signed"` where
   herald's compare-and-set record of the update names the head it was
   taken on and the base's head and says the forge found, right before
   the call, that they merge without a conflict, and the update was made
   or read back as made; right after the call herald reads the pull
   request's head with its read credential, and where it has not moved
   yet, again every 2 s while those pauses come to no more than the
   queue's `update_wait` ("10s" where absent, "0s" reads it once, at most
   "1m"), each read within the run's budget; it records the first head it
   reads moved, and reads no other, in the same compare-and-set record
   only where that head itself has exactly those
   two as its parents in that order and the forge itself signed it, as
   GitHub signs a commit it makes for `noreply@github.com` on github.com,
   or for the committer a queue's `signer` names on GitHub Enterprise
   Server and GHE.com, an identity the operator vouches is the instance's
   own and no user's. The gesture vouches for that recorded head alone,
   proved again the same way right before each later step. No forge's
   answer to the update names the new head (GitHub's update-branch answers
   202 with a message and a URL, GitLab's rebase says
   `rebase_in_progress`, Forgejo's update answers nothing), so herald reads
   it. GitHub also signs a commit a GitHub App's token, an Actions token
   among them, creates through its API with no author or committer given,
   and that API takes any tree and any parents: between herald's update
   call and its read of a moved head, one holding such a token with
   contents write could place a commit with those two parents and another
   tree, and herald would record it. That window runs from herald's update
   call until herald reads a moved head, at most `update_wait` of pauses
   and its reads' round trips; a commit pushed after that read is never
   vouched for, and a head no read within `update_wait` finds moved
   records nothing, so the pull request leaves as `head-moved` once it
   moves. GitLab's rebase and
   Forgejo give no such proof (Forgejo's API cannot tell the instance's
   signature from an author's), so there it vouches for none. `"never"`: the pull request leaves as `head-moved`
   and a new gesture queues it. `"approved"`: where herald's record names
   its update and one holding `least` approved the new head. A signed merge commit with those parents is no
   proof alone, as GitHub's web conflict editor makes one holding what its
   author typed; two heads that merge without a conflict have one merge
   alone, so herald brings a branch up to date only where the forge says
   it merges without one (`conflict` where it does not; it waits while
   the forge works it out). The method
   is taken only where the repository's settings, read right before the
   merge, allow it.
8. **Up to date by merging the base in.** `[sources.<name>] update =
   "merge"` (the default on GitHub and Forgejo) merges the base into the
   branch through the forge, which keeps the author's commits and their
   signatures; `"rebase"` rebases it through the forge. GitLab offers a
   rebase alone: its default is `"rebase"`, and `"merge"` does not load
   there. A fork whose author does not let maintainers edit cannot be
   updated: it leaves the queue (`cannot-update`), the author's turn.
9. **Green.** By default herald merges only where every check reported
   on the head has passed (GitHub's check runs and statuses, GitLab's
   head pipeline and each commit status on the head outside it by its
   own name, one GitLab allows to fail counting as passed, Forgejo's
   statuses), every check the forge requires
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
    queue's own, as GitHub's merge queue rule names it (`"repository"`
    taken as the repository's one allowed method), and a `method` naming
    another does not start; GitLab names no method of its train's own.
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
    then reads that queue's write credential, and doctor says so, and
    says on every layer that does not run `herald server` that no step
    of that queue is taken there. `"command"` is the default.
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
    one record per gesture, keyed by the forge's API, the repository,
    the pull request's number and the forge's id of the label event or
    comment. It also holds who made the gesture and when, compared on
    every read: where the forge names another maker or time for the id,
    it is not the same gesture and nothing is written for it (taking
    out compares the time alone, so a deleted account's gesture still
    counts as the role lost). A gesture fixes the head it
    vouches for, so this is one record per pull request, head and
    gesture. It holds the head first read with the gesture standing and
    each step (the head, the credential's name, the base's head, time,
    outcome), written with compare-and-set before the call, as
    ADR-0068's re-run record is: of two runs racing only the first
    calls, and a crash after the record leaves a step no run takes
    again. Only a step whose request provably never left herald is taken
    again, after a fresh read. A lost answer is never asked again:
    herald reads the pull request back and records what came of it. The
    record also holds each take-out of point 6, keyed by the cancel's id,
    who made it and when (none for the role lost), written the same way
    on the anchor's record: one take-out per cancel, and a new cancel
    after a new gesture is another. Beside them the port keeps one
    record per pull request, keyed by the forge's API, the repository
    and the number, of the last time it left herald's own queue: when,
    why and the gesture's id, never a title, a body or a credential. It
    is written once the gesture's record says it left, and again by a
    later run where that write failed; every view puts it on the pull
    request's row as `queue.left`. Only
    `herald merge-queue --apply` (or the server, with `applied_by =
    "server"`) writes these; the dry run reads them. The markers store keeps
    no records, so a queue on it does not start.
15. **The setup wizard sets every key.** `herald setup merge-queue`
    asks for each key in turn, with its default chosen and the forge's
    own answers shown (whether the branch has a queue and why not, the
    method it merges by, whether it merges only through it, the methods
    the repository allows), offers `runs = "herald"` where the forge has
    no queue on the branch herald can hand a pull request to, offers only
    the methods the repository allows (with `runs = "forge"`, the forge
    queue's own), defaults `updated` as the config does, and writes the
    `[merge_queues.<name>]` table into the config once the operator
    confirms it.
16. **Doctor names, for each queue,** the forge and whether it has a
    queue on the branch herald can hand a pull request to, or lets one
    merge there only through it, the method and whether the repository
    allows it and, with `runs = "forge"`, whether it is the forge
    queue's, which head a gesture vouches for and its `skew`, the update style, whether the write credential is set and held
    apart, whether the store keeps records, and where the steps run.
17. **Every forge's own host.** Each read and step goes to the source's
    configured API base: github.com, a GHE.com tenant, GitHub Enterprise
    Server (whose merge queue serves organization repositories; a server
    whose GraphQL lacks `enqueuePullRequest`, as herald reads its schema
    with the read credential where the branch has a merge queue, takes no
    `runs = "forge"`, and herald never calls `merge-async`), GitLab.com, Self-Managed and
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
