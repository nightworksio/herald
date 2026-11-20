# ADR-0065: A view triages failing checks, and a rule plans a re-run of a flaky one

**Status:** Accepted
**Date:** 2026-11-20

## Context

A check failing on many open pull requests at once says their base is
broken, not that each author must act. A check that fails and then passes
on the same commit is likely flaky, and maintainers re-run it by hand. The
user decided that triage and flaky candidates are read-only, and that a
re-run is planned by a rule and made only by an operator signed in through
the forge, never by a rule on its own.

## Decision

1. **Triage on every board.** A board names each check failing on the pull
   requests it shows, by name, those failing on the most first, with the
   pull requests it fails on. A check failing on `[views.<name>]
   broken_at` (3) of them marks their base as broken. Templates read them
   as `view.failures`.
2. **Flips are kept.** A kept record notes each time one of its checks
   failed and then passed on one head: from a check delivery following a
   failure on that head, or from a read of the same head finding passed
   what the last read found failed. It keeps the newest 20.
3. **Flaky candidates.** A failing check that flipped `flaky.least` (1)
   times within `flaky.window` ("7d") across a view's pull requests is a
   flaky candidate. Live reads keep no history, so they name none; doctor
   says so for a rule reading a source live.
4. **A rule plans a re-run.** `[rules.<name>] flaky = { rerun = true }`
   plans, at most `most` a window, a re-run of each flaky check failing on
   its view's pull requests. `stages` is optional; a rule nudges, plans,
   or both. `mode` is "plan"; "apply" is refused at load.
5. **A plan names the call.** Through the source that reads the pull
   request's repository, a plan reads the failed runs on its head that ran
   the check: GitHub Actions' runs whose job of that name failed, or
   GitLab's failed pipelines, the newest `flaky.runs` (5, at most 100). It
   names each with the exact call that re-runs its failed jobs. Forgejo has no API to re-run a run, so its
   plan says to re-run it by hand.
6. **Plans reach private routes alone.** A plan is a `rerun` event,
   identified by rule, pull request, check, head and run, so it is
   announced once a run and head, to the routes naming the rule that say
   `private = true`. herald ships `herald:rerun`.

7. **Every request is charged as it is made.** A view's read, its
   reconcile and a plan reach the forge through one HTTP handle that asks
   the run's budget before each request, so a read taking many requests
   stops at the host's limit with the budget's reason, making none past
   it.

## Consequences

- A plan costs at most `runs + 1` requests on GitHub and one on GitLab;
  doctor names the bound for each rule.
- Making the planned call, behind an operator's forge sign-in, a named
  write credential and checks immediately before the call, is a decision
  of its own.
