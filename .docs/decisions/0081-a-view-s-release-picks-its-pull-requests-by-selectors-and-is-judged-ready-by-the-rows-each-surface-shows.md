# ADR-0081: a view's release picks its pull requests by selectors, and is judged ready by the rows each surface shows

**Status:** Accepted
**Date:** 2026-11-23

## Context

A maintainer cutting a release asks one question of their queue: is it
ready, and if not, what holds it back. The pull requests of a release are
told apart in several ways: the milestone they carry, a label, the
branch they merge into, or the release pull request a tool such as
release-please or the changesets action opens. Teams differ in which
they use and in what they take as a blocker. Since ADR-0061 every reader
and every kept record names each pull request's milestone, base branch
and head branch, so a view can pick and judge them at no request of its
own.

Every row a surface shows follows its repository's visibility
(ADR-0062, ADR-0069): a public route is shown only public rows, and
`herald queue` withholds a private one's unless said private. A verdict
is a fact about rows, so it may leak a hidden row as surely as the row.

## Decision

1. **`release` on a view picks its pull requests.** `release =
   { milestone, label, branch, pull, combine }` names at least one
   selector: `milestone` a glob on the milestone title a pull request
   carries, `label` a glob on any label it carries, `branch` a glob on
   the branch it merges into, `pull` the release pull request:
   `"release-please"` (head `release-please--branches--*`),
   `"changesets"` (head `changeset-release/*`), `{ head = "<glob>" }` or
   `{ title = "<glob>" }`. `combine = "all"`, where absent, picks a pull
   request each selector picks; `"any"` one any picks. A release naming
   no selector is refused. The view shows only what its release picks,
   beside its other filters.
2. **What holds a release back is a setting.** `blocked_by` names the
   blockers, every one where absent: `label`, a pull request carrying a
   blocker label (`blocker_labels`, globs, `["blocker",
   "release-blocker"]` where absent, at most 50); `failed`, one whose
   check failed; `draft`, a draft; `changes`, one a reviewer asked to
   change. A pull request listed past `most_pulls`, whose checks and
   reviews are not known, holds it back whatever `blocked_by` says, so a
   release is never said ready on facts unread.
3. **A verdict, its reasons and a JSON key.** The board says `ready`, or
   not ready with one hold for each pull request and blocker, naming the
   blocker labels it carries or the checks that failed. Templates and
   `herald queue --format json` read `view.readiness = { ready, holds:
   [{ reason, repository, number, url, named, more }], more }`; the
   table prints `release: ready` or `release: not ready, <n> reasons`
   and a line a hold. At most 100 holds are named and at most 10 checks
   or labels a hold; those past each are counted.
4. **Judged by the rows a surface shows.** The verdict is computed where
   a board's rows are grouped, from those rows alone: each route of a
   report from the rows its audience is shown, `herald queue` from the
   rows it prints. A private repository's pull request never reaches a
   public route's verdict, nor `herald queue`'s output without
   `--private` or `[cli] private`; there the table says the verdict is
   by the pull requests printed, beside the count withheld.

## Consequences

A release board costs no request beyond the view's own reads. A public
route may be told a release is ready while a private repository's pull
request holds it back: it is told nothing of what it may not see, and
the private route beside it is told the whole verdict. A board's header
of the milestone itself and its base branch's checks need reads of their
own and are not part of this decision.
