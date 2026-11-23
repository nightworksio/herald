# ADR-0079: a milestone is a versioned forge signal a poll finds by following the open ones

**Status:** Accepted
**Date:** 2026-11-23

## Context

Teams that release by milestone ask herald to announce one when it
closes, as they would a Release. GitHub sends a `milestone` webhook
(`created`, `closed`, `opened`, `edited`, `deleted`) and lists a
repository's milestones by state, but neither filters nor sorts them by
update. GitLab sends its Milestone Hook from 18.2 (`create`, `close`,
`reopen`), names no issue counts in it, and lists a project's and a
group's milestones by state. Forgejo and Gitea send no milestone webhook
at all: their `issue_milestone` is an issue milestoned or demilestoned,
delivered as an `issues` event. A poll that lists every milestone cannot
find one just closed within a bounded page on GitHub or Forgejo.

## Decision

1. **A kind and a signal.** Kind `milestone`, versioned: its `version` is
   its title and its `prerelease` the title's pre-release part, so a
   route's `versions` and `prereleases` take milestones as they take
   Releases. Signal `milestone`, off unless `signals` names it. It
   carries `action`, `number` (GitHub's number, GitLab's iid, Forgejo's
   id), `milestone` (its title), and where known `due`, `issues_open` and
   `issues_closed`; its `milestone` link opens it.
2. **Every transition is a setting.** `milestones.actions` takes
   `opened`, `closed`, `reopened` and `deleted`; `closed` where absent.
   `milestones.titles` takes globs. A deletion withdraws the opening or
   closing it names from each route that withdraws (ADR-0048), and only a
   webhook delivers one.
3. **What it says.** Its title is `Milestone <title> <action>: <closed>
   of <total> closed, due <date>`, counts and due day where known; its
   body is its description, cut to 4 KiB on a character.
4. **The forge's own counts.** `milestones.counts`, true where absent.
   GitHub and Forgejo carry them in the milestone (GitHub's count pull
   requests too). GitLab's count issues only and cost one request per
   milestone a poll announces; its webhook names none, so a milestone it
   delivers carries none.
5. **A poll follows the open milestones.** Each round asks each listing
   of open milestones conditionally, a page at a time up to
   `milestones.most` (100 where absent, at most 1000), keeping their ids
   in the cursor (`Cursor.ids`, by listing). An id that appears is
   opened, or reopened where the cursor saw it close or the forge names a
   closing; an id that left is read once by its own path: closed is
   announced, still open (past `most`) or gone is not. A first poll only
   follows. GitLab's group milestones are a second listing where
   `milestones.group` says so, named on the group's id.
6. **One visibility rule.** A milestone event is born with its
   repository's origin, so a private or internal repository's milestone
   reaches only private routes while `[doctor] private_to_public` refuses,
   as every private repository's event does. A GitLab group milestone is
   read through a project and bound to that project's visibility, which
   GitLab never lets exceed its group's.
7. **Every read is bounded and rate-limited.** Each listing, read by id
   and count goes through `limited::checked`, so a rate limit stops the
   poll as one; each answer is parsed under the source's `parse`, a count
   past `u32` is unreadable, and a due day is read as a date.
8. **The Actions layer.** `announce --from github` reads a run's own
   `milestone` event as a webhook's.

## Consequences

A milestone closed on any forge is announced within one round of a poll,
whatever the repository's history, for one conditional request a round
while nothing changes. Forgejo and GitLab before 18.2 rely on the poll,
and doctor says so. Doctor names each forge's counts and their cost.
