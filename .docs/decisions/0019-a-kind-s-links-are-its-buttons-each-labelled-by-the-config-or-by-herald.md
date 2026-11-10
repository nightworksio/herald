# ADR-0019: A kind's links are its buttons, each labelled by the config or by herald

**Status:** Accepted
**Date:** 2026-11-10

## Context

ADR-0018 gives every kind the names of the links that become its buttons.
Discord drew two buttons whatever the kind, `release` and `changelog`, each
labelled by a phrase the config may word in its own language (ADR-0016).
A declared kind names links of its own, such as a deploy's dashboard or
status page, and no phrase labels them.

## Decision

1. **A kind's links are its buttons, in its order.** An announcement carries
   a button for each link its kind names that the event carries, and a
   destination draws them in that order before its own buttons, such as
   subscribe.
2. **A declared link is a name, or a name and a label:**

   ```toml
   [kinds.deploy]
   links = ["dashboard", { name = "status", label = "Status page" }]
   ```

   A label is 1 to 75 characters, the most a button takes on any
   destination.
3. **A button is labelled by the config's label, else by herald's phrase for
   its link, else by its name.** herald words three links: `release`
   (`release_link`, "Release"), `changelog` (`changelog_link`, "Changelog")
   and `entry` (`entry_link`, "Read"), each worded in `wording` like any
   phrase. A feed entry's post carries a button to its entry.

## Alternatives considered

| Option | Why not |
|--------|---------|
| **The label is always the name** | `dashboard` reads as a key, not a label, and cannot be worded in the channel's language. |
| **Labels in a map beside the links** | The order and the labels would live in two keys that can disagree. |
| **No button for a feed entry** | A feed entry has a link like a release has, and a button to it is the same affordance. |

## Consequences

**A feed entry's post gains a "Read" button on Discord.**

## Related

- [ADR-0016](0016-commands-are-carried-every-way-a-destination-offers-and-answered-from-what-herald-keeps.md): the wording phrases
- [ADR-0018](0018-a-kind-is-data-and-an-event-carries-its-kind-s-attributes.md): a kind's links, extended here
