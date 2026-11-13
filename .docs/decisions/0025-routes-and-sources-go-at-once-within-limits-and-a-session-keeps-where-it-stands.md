# ADR-0025: Routes and sources go at once within limits, and a session keeps where it stands

**Status:** Accepted
**Date:** 2026-11-13

## Context

A run announced an event to one route after another, and a poll read one
source after another. One slow or rate-limited destination held up every
other route, and one slow source every other source. Only an ActivityPub
actor's deliveries to its followers' inboxes went out at once (ADR-0020).

ADR-0005 keeps an announcement from being sent twice with a claim and a
compare-and-set before each step. Each route's announcement has its own key,
so its claim and steps are its own. Three stores do not take writes to two
records at once: `git-branch` commits every record to one branch,
`repo-variables` holds one lock for every write, and `actions-cache` saves
every record in one document. On those, a write made beside another is
refused as a conflict. A conflict after a step was sent leaves the step
unrecorded, and a destination that cannot be read back may then be sent it
again. The `markers` store compares and sets nothing, and two runs on it are
told apart only by the destination's own idempotency.

A Matrix sync kept where it stood in memory alone. A server that restarted
started a new sync, and the commands given while it was down went
unanswered.

## Decision

1. **`[run] routes` and `[run] sources`**, each 4 where absent and at least
   1, set how many routes an event is announced to at once and how many
   sources a poll reads at once:

   ```toml
   [run]
   routes = 4
   sources = 4
   ```

   1 announces to one route after another in the file's order, and reads one
   source after another.
2. **A store says whether it writes one record at a time.** `git-branch`,
   `repo-variables`, `actions-cache` and `markers` do, and on them routes and
   sources go one at a time whatever `[run]` says. Every claim, step and
   cursor is still written with ADR-0005's compare-and-set, so a run that
   goes at once sends nothing twice.
3. **What a project last announced is kept route by route, in the file's
   order**, once every route has answered, so routes going at once do not
   write it at once.
4. **The store keeps where each session stands**, one value per
   destination, the last written kept, as `positions/<destination>`. Every
   store that keeps cursors keeps positions too; `markers` keeps none. A
   Matrix sync keeps its `since` before it takes up the commands a sync
   brought, and a sync taken up again starts there.
5. **`[server.sessions] stale`**, "10m" where absent, bounds how old a
   command a session taken up again answers. A command given longer ago is
   skipped and logged.

## Alternatives considered

| Option | Why not |
|--------|---------|
| **Routes and sources at once, bounded by `[server] jobs`** | `jobs` bounds the server's pieces of work, and the CLI and every serverless host announce too. |
| **Routes and sources one after another, as before** | One slow destination holds up every other. |
| **Keeping the sync's position as a cursor under a reserved source name** | A cursor is what a source has seen; a narrow record per kind keeps each store's capabilities read from what it answers. |
| **Answering every command a sync brings, however old** | A member asking an hour ago is better left unanswered than answered late. |

## Consequences

**A rate-limited destination holds up only its own routes**, while as many
others as `[run] routes` says go on.

**The stores that write one record at a time announce as before**, one route
after another.

**A restarted server answers the commands given on Matrix while it was
down**, as long as they are younger than `[server.sessions] stale`.

## Related

- [ADR-0003](0003-every-source-becomes-one-neutral-event.md): the sources a poll reads
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the claim and compare-and-set every route keeps, and the records a store holds, extended here
- [ADR-0016](0016-commands-are-carried-every-way-a-destination-offers-and-answered-from-what-herald-keeps.md): the sessions that carry commands, extended here
- [ADR-0020](0020-an-activitypub-destination-sets-how-many-inboxes-it-delivers-to-at-once.md): the deliveries that already went at once
