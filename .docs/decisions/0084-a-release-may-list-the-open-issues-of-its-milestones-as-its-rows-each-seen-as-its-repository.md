# ADR-0084: a release may list the open issues of its milestones as its rows, each seen as its repository

**Status:** Accepted
**Date:** 2026-11-24

## Context

ADR-0081 shows a release's board as the open pull requests it picks, and
ADR-0082 heads it with the open milestones its milestone selector
matches. A team that plans a release by its milestone's issues wants
those issues as the board's rows: what is still to be done, not what is
under review. The issues are no fact of any pull request; listing them
takes requests of their own, and GitHub lists issues by a milestone's
number, which only the milestone listing says.

## Decision

1. `release.rows` says what the board lists: `"pulls"` where absent, the
   pull requests the release picks, or `"issues"`, the open issues
   carrying each open milestone its `milestone` glob matches. `"issues"`
   needs a milestone selector; a release without one is refused.
2. The issues are read with the header, from the same repositories and
   the same milestone listing (ADR-0082): one more request a round for
   each milestone matched, GitHub by its number with pull requests left
   out, GitLab by its title, Forgejo by its id, one page of the source's
   `page` each, at most 50 rows a repository. With `header = false` the
   listing is still read for the issues, and no milestone is named nor
   counted.
3. Every request goes through the rate-limit check. A refusal names the
   token permission it needs on each forge and its enterprise or
   self-managed variant; a 404 names the repository unseen. A page the
   listing fills, and a repository past the 50, are named unread with
   what was left out. None of these is shown as "no issue".
4. Each issue is seen as its repository, and a confidential GitLab issue
   as private. They are filtered with the header: `Overviews::seen_by`
   for each report audience, `Printed::overviews` for `herald queue`,
   `within` for the view's repositories, and they count in a read's
   strictest visibility.
5. The release's pull requests still judge whether it is ready: an open
   issue holds nothing back. A template reads `view.issues`, none where
   the board lists pull requests; the `herald:queue` template and the
   `herald queue` table list them in place of the pull requests.
6. A board judged beside no named audience sees the least: the base
   branches a surface is judged beside are seen as public unless the
   surface's audience is named, so a private repository's base branch is
   never told to an unnamed one (amends ADR-0083).

## Consequences

A release listing issues costs one more request a round for each
milestone matched, at most 10 a repository and 20 repositories. A public
route is never told a private repository's issues, nor a confidential
issue. An open issue holding a release back, and a milestone spanning an
owner's repositories, are not part of this decision.
