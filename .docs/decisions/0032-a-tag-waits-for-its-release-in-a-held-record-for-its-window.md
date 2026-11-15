# ADR-0032: A tag waits for its Release in a held record, for its window

**Status:** Accepted; a CI run holding a tag extended by [ADR-0033](0033-a-ci-run-for-a-tag-announces-the-tag-or-holds-it.md)
**Date:** 2026-11-15

## Context

A source can announce both the tags and the Releases of a repository, and
a tag and a Release of it share one identity, so a route announces the
version once, whichever comes first (ADR-0031). A Release usually follows
its tag by minutes, and its notes say more than a tag's message, so the
version reads best announced as its Release. A tag that never gets a
Release must still be announced. Waiting needs a record that outlives the
run that saw the tag, and the same need comes back when a route groups the
announcements of a release train.

## Decision

1. **`prefer` chooses between a tag and its Release** where a repository's
   signals take both: `"release"` (the default) holds a tag with no Release
   for `tag_window` ("1h" where absent, at most "7d"), and `"first"`
   announces whichever comes first. Either key on a source that takes
   both signals for no repository does not load, and so does `tag_window`
   beside `prefer = "first"`.
2. **A held record keeps the tag.** A new kind of record, `Pending`, keeps
   events held back until they are due, under what holds them: here
   `Hold::Tag { source, identity }`, with the event, the owner's member it
   is of, when it was first held and when it is due. A grouped
   announcement's hold becomes another `Hold`. Every store that keeps
   records keeps it, written with compare-and-set; the markers store keeps
   none.
3. **The window counts from when herald first sees the tag**, so a tag on
   an old commit still waits. A poll that finds a fresh tag holds it,
   unless it is held already, and its cursor remembers the tag as seen.
4. **A Release lets its tag go**: a fresh Release drops the record of its
   tag and is announced at once. A cursor remembers which identities it has
   seen only as a tag, so a Release of one of them is fresh, through REST
   and through GitHub's GraphQL batch alike.
5. **Each poll round and each housekeeping round announces what is due**,
   each held event to the routes that take it, as its source's. A member's
   event is bounded by its membership record as a polled member's is, and
   let go unannounced where the record no longer holds the member. The
   record is dropped once every route has the event, and kept for the next
   round where a route stopped. A held record that cannot be read or
   written stops the source for the round.
6. **A webhook of a tag polls its source.** GitHub's and Forgejo's
   `create` of a tag and GitLab's tag push that deletes none, of a
   repository the source takes and announces tags of, are answered at once
   and leave a poll of that source to the work after, which reads the tag
   in full and announces or holds it as any poll does; every owner is
   still listed, so each member's project is checked against every other's.
   A delivery of a tag the source announces no tags of announces nothing.
   This replaces ADR-0031's rule that a webhook announces releases only,
   and doctor no longer notes it.
7. **Doctor names the gap** where the markers store serves a source that
   holds tags: "holding a tag for its Release", keyed by the source's
   `prefer`.

## Consequences

- A release train announces each version once, as its Release where one
  follows within the window, and as its tag otherwise.
- A tag waits up to its window plus one poll or housekeeping interval.
- Grouping (`group = { ... mode = "hold" }`) adds a `Hold` and reuses the
  record, the rounds that announce what is due, and the stores.
