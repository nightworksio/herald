# ADR-0045: A route gathers the events that share a key into one announcement

**Status:** Accepted
**Date:** 2026-11-17

## Context

A release train tags many repositories with one version within minutes,
and the core's Release often comes later. Announced one by one, a train of
17 repositories is 17 posts. A route can name a key that the train's events
share, such as their version, and announce them as one. ADR-0032 records a
held event in a `Pending` record until a round announces it, and ADR-0034
edits a post with the steps a destination offers. Grouping reuses both.

## Decision

1. **`group = { key, window, lead, mode, until, late, most }` on a route.**
   `key` is a one-line template, checked at load against every kind the
   route takes. An event whose key renders empty or longer than 256 bytes
   is announced alone. A group is per route, and its members may come from
   any of the route's sources. Its announcement is keyed by
   `group/<key>` on the route.
2. **`mode = "hold"` (the default) holds the members** in a
   `Hold::Group { route, key }` record. It gathers for `window` ("30m"
   where absent, at most "7d"), counted from the first member herald sees.
   With `until = "lead"` the group is announced as soon as its lead comes;
   with `until = "window"` (the default) only when its window passes. Each
   poll and housekeeping round announces a group that is due, and a member
   that comes once the group is due is announced with it at once. Before a
   held group is announced, its record is closed to new members, so each
   member is either in the post or late.
3. **`mode = "edit"` posts the first member at once and edits the post** as
   each other member comes within the window, with ADR-0034's steps. A
   route whose destination cannot edit (a Slack incoming webhook) does not
   load with `mode = "edit"`.
4. **`lead` names the projects whose event leads a group,** as globs that
   ignore case. The first glob that matches a member wins, and the first
   member leads where none matches. The others follow in the order they
   came. Every template reads `events` (the members, lead first, or only
   the event where the route does not group) and `group` (with its `key`,
   and none where the route does not group). `event` is the lead. A group
   of one is still announced under its group's key.
5. **A member that comes after its group was announced is late:** for a
   held group, once its record closed; for an edited one, once its window
   passed. `late = "post"` (the default) announces it alone. `"edit"` edits
   it into the group's post where the destination edits, and announces it
   alone otherwise. `"skip"` leaves it unannounced.
6. **Each member's ledger entry records the group it was announced in**,
   with a digest of its title and body, so a webhook, CI run or app sending
   it again never announces it alone. A Release of a member the group shows
   as a tag, or an edit of a member's Release, re-renders the group and
   acts on its post as the route's `late_release` and `edits` say
   (ADR-0034). The group's record is kept as long as its ledger entry
   (`keep_ledger`), counted from when it closed, or from its window for an
   edited group. After that, a late Release is posted anew.
7. **`most` caps a group's members** (25 where absent, at most 100). A
   member past it, or one that would take the group's record past what
   the store keeps in a record, is announced alone. Doctor notes a route
   whose `most` passes the members the store's records hold with long
   notes (8 KiB each).
8. **A route mentions the subscribers of every member's project,** each
   once, and keeps what each member's project last announced. A route that
   announces each project as an actor of its own (ADR-0036) does not load
   with a group, and the error suggests a second route on a destination
   with one actor. A route to a herald feed (ADR-0046), which publishes
   each event as an envelope of its own, does not load with a group
   either.
9. **The markers store keeps no records**, so a grouped route there stops
   for the round, as a held tag does (ADR-0032), and doctor names the gap
   "grouping a route's announcements" at the route's `group`.
10. **A member reaches a group only where its visibility reaches the
    route** (ADR-0029): a private member is refused before it is gathered,
    and one that names no visibility of its own is held under the bound
    the routing gave it. Each flush leaves out a member the config no
    longer lets reach the route, and a group left with none is announced
    to no one. The group is announced under its strictest member's
    visibility.
11. **`herald preview --group <key>`** renders each route that groups
    against the event file's release, or every sample release, as one
    group.

## Consequences

- A train is one post, held for its window or edited as it grows.
- A held group's post waits for its window plus one round, unless its lead
  closes it.
- Every template renders both a group and an event alone, as a route that
  groups still announces some events alone.
