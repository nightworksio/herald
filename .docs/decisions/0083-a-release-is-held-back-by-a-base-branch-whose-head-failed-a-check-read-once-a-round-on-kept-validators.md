# ADR-0083: a release is held back by a base branch whose head failed a check, read once a round on kept validators

**Status:** Accepted
**Date:** 2026-11-23

## Context

ADR-0081 judges a release by its pull requests alone, and ADR-0082 heads
it with its milestones. A release is cut from the branch its pull
requests merge into: where that branch's own head fails its checks, the
release is not ready however clean each pull request is. That state is
no fact of any pull request; reading it takes requests of its own, a
round after round on a branch that rarely moves, and it is as private as
the repository holding it.

## Decision

1. **`base` is a blocker, in the default list.** `blocked_by` names five
   blockers, every one where absent: `label`, `failed`, `draft`,
   `changes` and `base`. With `base`, each branch a pull request the
   release picks merges into holds the release back where a check failed
   on its head. Without it no branch is read.
2. **Read once a round per repository and branch.** Each round, the
   head of each such branch is read once: on GitHub, GitHub Enterprise
   Server and GHE.com its check runs and its combined commit status (two
   requests); on GitLab, self-managed included, its newest pipeline, and
   where it failed or was cancelled one more request for its failed jobs,
   those allowed to fail left out; on Forgejo its combined status. Every
   request goes through the source's API setting and the run's budget,
   and a rate limit stops the read as every forge read does. At most 20
   branches are read a view; each past it is named unread.
3. **Asked on kept validators.** Where the store keeps pull requests,
   each request's answer is kept with the validators it came with, one
   record per source, repository and branch, in every store backend
   holding pull requests. The next round asks each request on them, and
   an unchanged answer is read as kept. A record of another form, or of
   another number of requests, is read again in full; one last read
   before the pull requests' expiry is forgotten with them.
4. **One line naming each failing check.** A failing branch is one hold:
   its repository, its branch and each check that failed, at most 10
   named and the rest counted, in a report's release section and in
   `herald queue`'s table. `view.overviews.bases` gives a template every
   branch read, passing or not.
5. **An unread branch holds the release.** A refusal, an unseen
   repository or branch, a rate limit, an unreadable answer or the run's
   budget ending names the branch unread with why, and it holds the
   release back as a pull request not read in full does: a release is
   never called ready on a branch nobody read. Doctor names the requests
   and the token's permission on each forge.
6. **Each branch is seen as its repository, and follows `withheld`.** A
   branch carries the visibility of its repository's rows. A surface
   that may not see it is not shown it, and its hold is told as ADR-0081
   tells a withheld pull request's: under `unknown` the verdict is not
   known where nothing shown holds the release; under `shown` it is
   judged without it; under `masked` it is one hold saying only that it
   is private.

## Consequences

A release reading its base branches costs at most two requests a round
for each branch on GitHub, one or two on GitLab and one on Forgejo, at
most 20 branches, and less where its answers are unchanged and kept. A
public route is never told a private repository's branch, its checks or
that it failed. Issues as rows and a milestone spanning an owner's
repositories need reads of their own and are not part of this decision.
