# ADR-0078: a pull request's merge queue entry is read with it and rides its row's visibility

**Status:** Accepted
**Date:** 2026-11-23

## Context

A view said only whether a pull request was in its forge's merge queue:
`Pull.queued`, set from GitHub's `isInMergeQueue` and its `pull_request`
`enqueued` and `dequeued` webhooks, and always false on GitLab. A
maintainer reading the queue asks more: where an entry stands, whether
its checks run, how long it waited, the forge's estimate, and why an
entry left. An entry that leaves for a failure is the author's turn, and
nothing announced it.

GitHub names an entry on the pull request itself (`mergeQueueEntry`:
position, state, `enqueuedAt`, `estimatedTimeToMerge`), readable in the
GraphQL query that already reads each pull request. Its `pull_request`
`dequeued` webhook names why an entry left, and `merge_group`
deliveries say when a group's checks were asked for and when a group was
destroyed. GitLab's merge trains are a separate listing per project,
Premium only, refused with 403 otherwise. Forgejo and Gitea have no
merge queue.

## Decision

1. **One home.** `Pull.queued: bool` becomes `Pull.queue:
   Option<QueueEntry>`: position, state (`queued`, `checking`,
   `mergeable`, `merging`, `unmergeable`), when it entered, the forge's
   estimate in seconds, and `left` (when and why it last left). Whose
   turn a pull request is reads "in the queue" as an entry with no
   `left`; the `queued` rule keeps its meaning.
2. **A view reads the queue where it says.** `queue = true` on a view,
   off where absent. On GitHub with a token the entry is read in the
   query that reads each pull request, at no request of its own; without
   it `isInMergeQueue` still makes a bare entry. On GitLab one request a
   project a round reads its active merge train cars, each entry's
   position counted on its target branch's train. A 403 reads as no
   train only where it is not the forge's rate limit; a rate limit stops
   the read as every other forge read does (ADR-0077). GitHub without a
   token and Forgejo read none, and doctor names why for each source,
   with each read's cost.
3. **Kept records fold the webhooks.** `enqueued` makes an entry,
   `dequeued` keeps the entry with `left` and GitHub's reason named as
   herald names it (merged, removed, cleared, checks-failed,
   checks-timed-out, conflict, invalid, protections, unknown); a
   `merge_group` delivery moves a waiting entry to `checking` when its
   checks are asked for and back to `queued` when the group is destroyed
   unmerged. A full read keeps the time an entry entered where the read
   names none, and the record of why an entry left until it enters again.
4. **The entry rides the row.** It is part of the pull request, so it is
   shown, withheld and counted exactly as its row is (ADR-0069): a
   private or internal repository's position, state and timing reach no
   public route, no table and no JSON without `--private` or a private
   route. No new visibility class and no new permission: the entry is
   read with the credential that reads the pull request.
5. **A rule alerts on leaving.** `[rules.<name>] dequeued = true`
   alerts once on each pull request of its view that left for a reason
   that is a failure of it (checks-failed, checks-timed-out, conflict,
   invalid, protections); a list names other reasons. The `dequeued`
   event carries the rule, the reason, whether it is a failure, when it
   left, the repository, number, author and bot, links the pull request,
   is seen as its repository is, and ships the `herald:dequeued`
   template. Its identity holds the time it left, so each departure
   alerts once.

## Consequences

- A view reading the queue on GitHub costs no request; on GitLab one a
  project a round. A GitHub Enterprise Server too old to name
  `mergeQueueEntry` refuses the query, so the setting stays off there.
- Why an entry left is known only from webhooks: a live view and a
  GitLab train show where entries stand, never why one left.
- The kept record's shape changes; herald has no release, so no record
  is migrated.
