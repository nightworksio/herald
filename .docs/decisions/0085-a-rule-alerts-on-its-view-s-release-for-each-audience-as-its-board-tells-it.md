# ADR-0085: a rule alerts on its view's release for each audience as its board tells it

**Status:** Accepted
**Date:** 2026-11-24

## Context

A view's release has a verdict (ADR-0081): ready, held back, or not
known where some of its pull requests are withheld from a surface. A
maintainer wants to hear when a release turns ready, and when it is still
held back as a milestone's due day nears, without opening the board. A
rule already watches a view once a window and alerts on what it finds
(ADR-0056, ADR-0078).

The verdict depends on who is told it. A private route is judged on every
pull request and base branch; a public route on the public repositories'
alone, beside the rest as `release.withheld` says. An alert that fired on
the private verdict would tell a public route, by firing, what a private
pull request decided.

## Decision

A rule names `release = { ready = true, late = <days> }`. Each window it
parts its routes into audiences as a report does: the private routes see
every row, the others the public repositories' rows. For each audience
the verdict is judged by the same call its board makes, beside the rows
withheld from it and the base branches it may see, and the header it may
see.

- `ready` fires when an audience's verdict is ready and the verdict it was
  told before was held back or not known. The first verdict a rule sees is
  only remembered. The rule's cursor keeps the last verdict of each
  audience, by the visibility it may see; a cursor from before reads as
  none told yet. Its identity names the audience and the window the cursor
  last remembered, so a window retried after a failed write is not told
  twice.
- `late` fires when an audience's verdict is held back, never when not
  known, on or after `late` days before the due day of the milestone due
  first that the audience may see, once for each milestone title and due
  day. It needs the release's header or issue rows to read the due days.

The event, of the kind `readiness`, is seen as its audience and announced
to that audience's routes alone. It names the view, the verdict, how many
holds the audience is told of, and the milestone it was judged late by;
never a repository, pull request or check it is not shown.

## Consequences

- A public route hears a release turn ready only where its board would
  say ready: never while `withheld = "unknown"` leaves it not known; with
  `"masked"` it counts the private holds without naming them.
- An audience's alerts depend only on what it may see, so adding a private
  pull request to a release changes no public route's alerts beyond what
  `release.withheld` lets that route know.
- The rule reads nothing more than the view's board does.
