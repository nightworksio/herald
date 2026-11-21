# ADR-0069: herald's commands withhold private repositories' rows unless their output is said private

**Status:** Accepted
**Date:** 2026-11-21

## Context

`herald queue` withheld a view's advisories unless `--private` said its
output was private (ADR-0067), but printed every pull request it read,
and `herald rerun` printed every plan. Both are run in CI jobs as well
as at a terminal, and a public repository's CI logs are public: a view
over an owner's repositories named its private and internal ones there,
with their pull requests' titles, authors and failing checks.

## Decision

1. **Withheld by default.** `herald queue` and `herald rerun` print a
   private or internal repository's pull requests, plans, CI runs and
   unread lines only where the output is said to be private. A
   repository whose visibility is not known counts as private.
2. **Counted, never named.** The table's last line and the JSON's
   `withheld_pulls` say how many pull requests were withheld; `herald
   rerun` says how many pull requests' plans were.
3. **Said private two ways.** `--private` on either command, or
   `[cli] private = true` in the config, off where absent. The setting
   releases rows and plans alone: a view's advisories are printed with
   `--private` only, as ADR-0067 decided.
4. **Apply reads what the plan prints.** `herald rerun --apply` reads the
   plans the same way, so a job of a withheld plan is refused as not
   planned, the refusal saying that `--private` or `[cli] private` would
   plan it, without naming its repository.
5. **One home.** `herald_core::Printed` (`Withheld`, `Configured`,
   `Asked`) decides, for rows and for advisories: `Printed::of` from the
   flag and the setting, `Printed::prints` per visibility, `advises` for
   advisories.

## Consequences

- An operator who reads a private repository at a terminal says
  `--private`, or sets `[cli] private` where every output stays private.
- A CI job printing a view over public repositories prints as before.
- Turning `[cli] private` on where a command's output reaches a public
  log names every private repository the view reads there.
