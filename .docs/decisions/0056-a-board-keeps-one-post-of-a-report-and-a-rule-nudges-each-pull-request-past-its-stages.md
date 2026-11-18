# ADR-0056: A board keeps one post of a report, and a rule nudges each pull request past its stages

**Status:** Accepted
**Date:** 2026-11-18

## Context

A report announces a view once a window (ADR-0055), so a channel fills
with one digest a window, and nothing tells the person whose turn it is
that a pull request has waited too long. An edit brings a post to what
it now renders (ADR-0034), a route keeps one pin (ADR-0011), and reports
are sources routes name (ADR-0055).

## Decision

1. **A living route keeps one post of each report.** A route with
   `living = true` names only reports among its sources. Its report's
   identity is `report/<name>`, with no window, so the first window posts
   it and each window after edits it, through the Edit step, where it
   renders otherwise. Where it says nothing of `pin` and its destination
   pins, it pins its post; `pin` also takes `true` (find the earlier pin
   by its marker) and `false`. Where its destination cannot edit, each
   window is posted anew, as a report's window is, and doctor says so.
2. **A board is a report and a living route at once.** `[boards.<name>]`
   takes a report's keys and a route's but `id`, `sources` and `living`;
   it is read as a report named and a route identified by the board's
   name, rendering `herald:queue` where it names no template. One
   implementation serves both forms.
3. **A rule nudges by stages.** `[rules.<name>]` watches a view once a
   window, with a report's `view`, `every`, `at`, `days` and `zone`, and
   names `stages` (1 to 16 lengths) and `most` (20). A rule's name shares
   the route source namespace with sources, apps and reports. Each window,
   each row of the view that reached a stage is nudged at the longest
   stage it reached, the longest waiting first, up to `most`; the rest
   wait for the next window.
4. **A nudge fires once a stage and spell.** A spell of waiting begins at
   the pull request's last update, the clock `stale` reads, until kept
   records tell its parties apart. A nudge's identity is
   `rule/<name>/<host>/<repository>/pull/<number>/<spell start>/<stage>`,
   so never-twice keys each stage of each spell.
5. **A nudge names logins only.** The `nudge` kind carries the rule, the
   stage, whose turn it is and why, how long it waited, the spell's start,
   the repository, the number, whether its author is a bot, its author,
   and the logins it waits on: its author's where it waits on them or is
   a draft, and each reviewer asked where it waits on review. Every forge
   now reads the reviewers asked. herald ships `herald:nudge`, which names
   them without mentioning anyone; mentions wait for `[people]`. A view's
   row gains `reviewers` and `waiting_on`. Only herald announces a nudge.
6. **A nudge is seen as its repository is.** A private or internal
   repository's nudge reaches only private routes while `[doctor]
   private_to_public` refuses.
7. **A view is read once a round.** Reports and rules watching one view
   share one read each round.
8. **Doctor** gives each rule the read path and cost rows a report gets,
   names its stages and its most, and names each living route whose
   destination cannot edit.

## Consequences

- A channel keeps one current, pinned queue, edited at most once a window,
  on every destination that edits.
- A nudge costs no request beyond its view's read, and at most `most`
  posts a window.
- A spell ends on any update, a comment or a bot's included, until kept
  records follow each party's own activity.
