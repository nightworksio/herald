# ADR-0048: A release the forge deletes is withdrawn from each route that asks

**Status:** Accepted
**Date:** 2026-11-18

## Context

A maintainer deletes a Release that was published by mistake, and its
announcement stays wherever herald posted it. The ledger (ADR-0005)
records every message a post sent, and ADR-0034 deletes a part of a post
that an edit no longer needs. ADR-0032 records held work in a `Pending`
record that rounds resume, and ADR-0045 gathers events into one group post.
Withdrawal reuses all four.

## Decision

1. **`withdraw = "keep" | "delete"` on a route, `keep` where absent.** A
   route that deletes withdraws a Release's posts once the forge deletes
   the Release. Only a Release deletion counts: GitHub's and Forgejo's
   release action `deleted`, and GitLab's release action `delete`, each
   read as the release's event marked `withdrawn`. Deleting a tag, a
   package or an image withdraws nothing. A poll that no longer finds a
   Release is not a deletion.
2. **A withdrawal deletes every message herald recorded** for the
   announcement, where the destination deletes: each chunk, the last
   first, then the post (`Step::Delete(0)`). Matrix takes a pinned post
   out of `m.room.pinned_events` before it redacts it, and Discord and
   Slack drop a pin as they delete its message. The route's pin record is
   forgotten where it names the post. A thread opened from the post is
   not deleted: where the route archives threads, housekeeping archives it
   at once. A crosspost stays.
   A destination that edits no post deletes none: the route keeps the
   post, and doctor names each route with `withdraw = "delete"` there.
3. **ActivityPub** delivers the Note's `Delete`, its object a `Tombstone`,
   to every follower's inbox that the Note's deliveries reached
   (`Step::Deletes`, kept apart from them under a key of their own). The
   Note is no longer served or listed in the actor's outbox.
4. **A herald feed** (ADR-0046) lists a tombstone in place of the
   release's envelope. The route forgets its post, so a re-created
   release enters the feed again.
5. **A withdrawn member leaves its group** (ADR-0045): the group's post is
   rendered again without it and edited. A group left with no member has
   its post withdrawn, and one that is still gathering drops the member
   from its record.
6. **Every post of the release on the route is withdrawn:** its own, the
   one a late Release posted anew (`#release`), and its place in a group.
   Each post's entry is marked `withdrawn`, not forgotten. A Release
   re-created after its `n`th withdrawal is announced anew under the
   identity `<identity>#<n>`: the first of its identities whose entry is
   not withdrawn.
7. **A withdrawal is durable.** It writes a `Hold::Withdraw { route,
   identity }` record, holding the forge's deletion, before its first
   step. It takes each step under the announcement's lease and the run's
   limits, as announcing does, and drops the record once every post is
   withdrawn. A run that stops part-way leaves the record, and each poll
   and housekeeping round finishes it. The markers store keeps no such
   record, so a withdrawing route there stops, and doctor names the gap
   "withdrawing a deleted release's posts" at the route's `withdraw`.
8. **What a project last announced falls back** (ADR-0016). A withdrawn
   newest release gives way to the stable release kept beside it. A
   withdrawn stable release is no longer kept. A project left with
   neither is forgotten.
9. **`herald withdraw <route> <identity>`** withdraws a release's posts
   from a route, whatever its `withdraw` says. `herald withdraw
   --every-route <identity>` withdraws them from every route, and
   `--dry-run` prints what each route would remove and sends nothing. A
   route that announces each project as an actor of its own (ADR-0036)
   withdraws only on the forge's deletion, which names the project.

## Consequences

- A Release deleted by mistake leaves no announcement behind on a route
  that deletes, and is announced again when it is published again.
- A withdrawal costs a ledger read for each event a withdrawable kind
  announces, to find the identity it is announced under now.
- A webhook destination keeps what it posted.
