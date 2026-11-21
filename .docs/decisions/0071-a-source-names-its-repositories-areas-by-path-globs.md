# ADR-0071: A source names its repositories' areas by path globs

**Status:** Accepted
**Date:** 2026-11-21

## Context

In a monorepo one queue holds the pull requests of many teams. A
maintainer wants to see the pull requests that touch the web app, the
docs or the API, and to group a board by them. ADR-0063 reads the paths
each pull request's head changes; areas name those paths. This decision
covers the areas a config names.

## Decision

1. **A source names areas.** `[sources.<name>.areas]` maps each area's
   name to the path globs any of which a changed path falls in it by,
   `web = ["web/**", "packages/ui/**"]`, written as `include` writes
   globs: `*` for any run of characters, `/` included, ignoring case. An
   owner's override, `[sources.<name>.repositories."<glob>".areas]`,
   gives the repositories it names their own areas in place of the
   source's, the narrowest override setting them winning as for every
   other overridden key.
2. **Bounded before it is read.** The table is checked as it is read
   into the config, by the one constructor of the core `Areas`: at most
   200 areas and 1000 globs, each name written without padding or
   control characters and at most 128 characters, each glob at most 256,
   and every area naming at least one. A changed path, which an author
   chooses, is matched only up to 1024 characters; a longer one falls in
   no area. The work matching one pull request is so bounded by the
   operator's table and the 100 paths ADR-0063 reads. A fuzz target holds
   the bounds.
3. **Reading areas reads paths.** A view reading a source that names
   areas reads each pull request's changed paths as `conflicts = true`
   does (ADR-0063): no request more on GitHub with a token, a request a
   pull request read in full elsewhere; doctor names that cost. A pull
   request's areas are those any of its paths falls in, in name order;
   one whose paths were not read falls in none.
4. **Views show, narrow and group by areas.** Each row and the template's
   row carry `areas`; `herald queue` writes them after the wait. A view's
   `areas` globs narrow it to pull requests falling in one of them, and
   each glob must match an area one of its sources names, or the config
   is refused. `group_by = "area"` puts a row in the group of each of its
   areas, and one in none in a group named by none; counts and totals
   count each pull request once.
5. **An area is its repository's data.** A row's areas travel with the
   row, so a private or internal repository's areas are withheld with its
   rows, by the one `Printed` (ADR-0069), from `herald queue` and every
   report not said private. Changed paths are printed nowhere.

## Consequences

- A board over a monorepo groups by area with no forge setting.
- A view over many sources may name an area only one of them has; a
  typo is refused rather than showing an empty board.
