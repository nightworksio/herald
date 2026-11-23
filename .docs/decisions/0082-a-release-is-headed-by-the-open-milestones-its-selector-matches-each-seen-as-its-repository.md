# ADR-0082: a release is headed by the open milestones its selector matches, each seen as its repository

**Status:** Accepted
**Date:** 2026-11-23

## Context

ADR-0081 judges a release by the pull requests its selectors pick, at no
request of its own. A maintainer asking whether a release is ready also
asks about the milestone itself: when it is due, whether that day has
passed, and how many of its issues are still open. Those are not facts
of any pull request: the forge holds them on the milestone, one per
repository, and reading them takes a request. A repository's milestone
is as private as the repository: its title, due day and counts tell an
audience that may not see the repository that the work exists.

## Decision

1. **A release with a milestone selector is headed by its milestones.**
   `release.header`, true where absent, heads the board with each open
   milestone whose title the `milestone` glob matches: its title, page,
   due day, whether that day ended before the day it was read on in UTC,
   and its open and closed issues as the forge counts them (GitHub and
   Forgejo count pull requests too). `header = false` reads none.
   `header = true` on a release with no milestone selector is refused.
2. **Which repositories are read, and how often.** Each round, each
   repository the view shows a pull request of the release in, and a
   one-repository source's own repository, is read once: one listing of
   its open milestones, one page of the source's `page` (30 where unset), and on
   GitLab one issue-statistics request for each milestone matched. At
   most 20 repositories are read a view, in name order within a source;
   each past it is named unread. At most 10 milestones are named a
   repository, and a page that filled or a match past 10 names the
   repository unread with what was left out.
3. **Each fact is seen as its repository.** A milestone, and a
   repository whose milestones could not be read, carry the visibility
   of the repository's rows, or private where it has none. A surface is
   shown only those it may see, as runs of the capacity overview are
   (ADR-0066): a public route the public repositories' alone, `herald
   queue` without `--private` those it prints. A header is no verdict:
   a milestone withheld from a surface changes nothing it is told of the
   release's readiness, which ADR-0081's `withheld` governs.
4. **A read that failed is named, never shown as no milestone.** A
   refusal, an unseen repository, a rate limit, an unreadable answer or
   the run's budget ending names the repository unread with why; the
   board's other reads go on. Doctor names the requests and the token's
   permission, for GitHub Enterprise Server, GHE.com and self-managed
   GitLab alike.

## Consequences

A release board costs one more request a round for each repository of
it, at most 20, and on GitLab one more for each milestone matched. A
public route is never told the title, due day or counts of a private
repository's milestone. The base branch's checks, issues as rows and a
milestone spanning an owner's repositories need reads of their own and
are not part of this decision.
