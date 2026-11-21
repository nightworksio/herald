# ADR-0072: A route narrows pull request events by the areas their paths fall in

**Status:** Accepted
**Date:** 2026-11-22

## Context

ADR-0071 names a repository's areas by path globs and shows them on a
view's rows. A team wants its own channel to hear only of the pull
requests that touch its area. A pull request's webhook names no paths,
so an event's areas come from paths read elsewhere: the record kept
mode keeps (ADR-0061, ADR-0063), or one more request to the forge.

## Decision

1. **An event's origin names its areas.** A pull request event's
   `origin.areas` lists the areas of its source's table for its
   repository that a path its pull request changes falls in, in name
   order, where every changed path is known. Where they are not, none
   read, more than one page of 100, a failed read, `origin.areas` is
   absent: its areas are not known.
2. **Kept paths by default.** The paths are those kept for the pull
   request where its kept head commit is the event's, which pull request
   events now carry as the `commit` attribute: no request more. A pull
   request whose record is not kept, or was kept for another head, has
   no areas. Doctor names, for each route narrowing by areas, each of
   its sources no view reads kept.
3. **Paths at delivery where a route says so.** `area_paths =
   "delivery"` on a route reads the paths from the forge as each pull
   request event is delivered: one request, one page of at most 100
   paths, through its source's token, API base (GitHub Enterprise
   Server, GHE.com, GitLab self-managed, Forgejo hosts) and the run's
   request budget, for the repositories its source reads, once for
   every route that asks. Off by default; doctor names the cost; a
   route asking for it with no areas to match is refused.
4. **Areas only narrow.** `match.areas` takes an event in one of the
   areas it names, each a glob that must match an area one of the
   route's sources names; an event whose areas are not known is not
   taken. Under `not`, an event in one of its areas, or whose areas are
   not known, is never taken. Areas never decide whether a route may
   ask for an event at all (`admits`), as attributes do not.
5. **Areas carry their repository's visibility.** They are part of the
   event's origin, so they reach only the routes the event itself
   reaches: a private or internal repository's event, and its areas,
   only the routes said private while `[doctor] private_to_public =
   "refuse"`. Paths are carried by no event. An app's or a peer's event
   may name at most 200 areas.

## Consequences

- A team's channel hears of the pull requests in its area with no
  request more where a kept view reads the source, and with one request
  an event where it asks.
- A pull request opened before any round read its paths is in no area
  until its record is read, unless its route reads at delivery.
