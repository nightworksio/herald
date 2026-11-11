# ADR-0020: An ActivityPub destination sets how many inboxes it delivers to at once

**Status:** Accepted
**Date:** 2026-11-11

## Context

ADR-0014 decision 3 lists `deliveries`, how many of an announcement's
inboxes are delivered to at once, among the limits of `herald server`, and
says the CLI honours it too. It is read by every command that announces,
whether or not a server runs, and it bounds one thing alone: an ActivityPub
actor's deliveries to its followers' inboxes. No other destination reads it.

## Decision

1. **`deliveries` is a key of a `kind = "activitypub"` destination**, 8 where
   it is absent and at least 1:

   ```toml
   [destinations.fediverse]
   kind = "activitypub"
   domain = "herald.example.org"
   key = { secret = "HERALD_ACTIVITYPUB_KEY" }
   deliveries = 16
   ```

   Every command that announces through the destination honours it, and
   each delivery is still one step of the ledger.
2. **`[server]` holds no `deliveries`.** herald has no release, so the key
   moves with no alias.

## Alternatives considered

| Option | Why not |
|--------|---------|
| **`[server] deliveries`, as ADR-0014 has it** | A key under `[server]` reads as a server's limit, and every command reads this one. |
| **A top-level section** | It would hold one key that only ActivityPub destinations read, and two actors could not differ. |

## Consequences

**Two ActivityPub destinations deliver at their own widths**, such as an
actor with many followers on a wider one.

## Related

- [ADR-0014](0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md): the server's limits, superseded in part here
