# ADR-0022: A read-back that fails stops the step, and pin lookups are bounded

**Status:** Accepted
**Date:** 2026-11-11

## Context

ADR-0005 has a run read back a destination's posts before it resends a step
an earlier run attempted, so an interrupted announcement is finished
without posting twice. Where that read failed, the run sent the step anyway,
and a destination that asked to wait was told nothing of it. ADR-0011 has
the unpin step look up each pin that could be the route's earlier one, one
request per pin, with no bound on how many.

## Decision

1. **A read-back that fails stops the step.** Where the destination asks to
   wait, the route stops for that long, as a step it refused with a wait
   does. Where its posts cannot be read, the route stops and says why. The
   next run reads back again.
2. **A destination's own identity is asked once** for as long as the
   destination is, rather than once per read.
3. **The unpin step looks up at most `pin_lookups` pins**, one request each,
   25 by default and at most 100. Where more pins could be the route's, it
   looks up none, refuses the step and says how to take them into account:
   raise `pin_lookups`, or unpin them by hand.

   ```toml
   [destinations.matrix]
   kind = "matrix"
   homeserver = "https://matrix.example"
   token = { secret = "HERALD_MATRIX_TOKEN" }
   pin_lookups = 50
   ```

## Alternatives considered

| Option | Why not |
|--------|---------|
| **Send the step when the read-back fails** | It can post an announcement twice, which ADR-0005 rules out. |
| **Look up the first `pin_lookups` pins and leave the rest** | An earlier pin past the bound stays pinned, and the step reports it unpinned. |
| **A fixed bound** | A room with many pins of its own needs more, and a small deployment fewer requests. |

## Consequences

**A destination whose posts cannot be read holds a resumed announcement
until they can be.** `herald doctor --online` names a Slack token that lacks
the scope to read them, and a Discord bot that cannot see its channel.

## Related

- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): reading back before resending, extended here
- [ADR-0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md): the unpin step, extended here
