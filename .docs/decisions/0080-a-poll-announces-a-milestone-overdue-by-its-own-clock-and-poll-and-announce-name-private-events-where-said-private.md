# ADR-0080: a poll announces a milestone overdue by its own clock, and poll and announce name private events where said private

**Status:** Accepted
**Date:** 2026-11-23

## Context

ADR-0079 announces a milestone opened, closed, reopened or deleted. Teams
that plan by milestone also ask to hear when one passes its due day
still open. No forge sends that: GitHub's, GitLab's and Forgejo's
milestone webhooks name changes to a milestone, and a due day passing
changes nothing. A poll's listing of open milestones answers 304 while
nothing changes, so the day passing is seen only from what a poll keeps.

`herald poll` and `herald announce` say what became of each event, a
line each, and run in CI jobs whose logs may be public. Since ADR-0079
they name a private or internal repository's event `(private)`, as a
server's log does, with no way for an operator at a private terminal to
see which event it was. ADR-0069 gave `herald queue` and `herald rerun`
that way: `--private` and `[cli] private`.

## Decision

This amends ADR-0079's decision 2 and extends ADR-0069's decision 3.

1. **`overdue` is an action of the milestone kind.** `milestones.actions`
   takes `overdue` beside the others; it is off where absent, which
   stays `["closed"]`. Its title is `Milestone <title> overdue: <closed>
   of <total> closed, due <date>`, its `occurred` the start of the day
   after the due day in UTC, its identity `.../milestone/<id>/overdue/<due
   day>`.
2. **Only a poll finds it, by its own clock.** A poll keeps the due day
   of each open milestone it follows (`<listing>/due`, `<id>:<day>`),
   from each listing page it reads and as kept where a page answers 304.
   On the first round whose time, in UTC, is past a due day, the poll
   announces the milestone once: from the listing where it was read this
   round, else read once by its own path, and only where it is still open
   with that due day. A due day that moves and passes again is a new
   identity, so it is announced again; a later poll of the same day is
   not.
3. **The passing is kept whether or not it is chosen.** The days already
   passed are kept (`<listing>/overdue`) whatever the source announces,
   and a first poll only records them, so choosing `overdue` later, or a
   new source, announces only the days that pass after. Recording one
   moves the cursor, which costs the round one read of the repository.
4. **Bounded by `most`.** Both kept sets hold only ids in the open set,
   which `milestones.most` bounds, and the passed set only days the due
   set holds; an entry that does not read as `<id>:<date>` is dropped.
   Each round reads by path at most the milestones whose day passed in
   it, each through `limited::checked`.
5. **Webhooks never carry it, and doctor says so.** A source choosing
   `overdue` is told that only a poll finds it, and that a source only
   delivered webhooks and never polled never announces it.
6. **Poll and announce output said private.** `herald poll --private`
   and `herald announce --private`, or `[cli] private = true`, name a
   private or internal repository's event by its version or title in
   their output; without either it is `(private)`. One home:
   `Event::named_in(Printed)`, `Event::named` being its withheld form.
   A server's, a Lambda's and a wasm host's log name it `(private)`
   always: they have no operator to say otherwise.

## Consequences

A milestone left open past its due day is announced within one round
after the day ends, at no request while its listing is unchanged and one
read when it is not in this round's listing. The cursor of a source
announcing milestones holds two more sets, each no larger than the open
set. An operator who knows their output is private sees which private
event a poll or an announcement handled.
