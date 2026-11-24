# ADR-0086: a release spans every repository its view holds, each seen as its own

**Status:** Accepted
**Date:** 2026-11-24

## Context

A release's header names the open milestones its milestone selector
matches, and may list their open issues as its rows (ADR-0082,
ADR-0084). It read only the repositories it showed a pull request of the
release in, and a one-repository source's own. A release of an
organization usually spans many repositories, most with no pull request
in the release yet, and on GitLab a release milestone is often a group
milestone that no single project holds.

An owner view mixes public, internal and private repositories. Its board
already shows each audience the rows of the repositories it may see
alone. The header must keep that: a public audience may not learn a
private or internal member's name, milestones or counts, nor that it
exists. A bound on what is read must not leak either: if a private
repository took one of the reads a public repository needed, a public
audience would see a public repository named unread for want of reads it
can count, and so learn that hidden repositories exist.

## Decision

- By default a release spans every repository its view holds: the
  repositories it shows a pull request of the release in, a
  one-repository source's own, and each member of an owner source.
- `release.repository = "<owner>/<name>"` holds the release to one
  repository, ignoring case: only its pull requests are the release's,
  and only its milestones and issues are read. The configuration is
  refused where the view does not hold that repository.
- On a GitLab owner with `milestones.group = true`, the header also
  reads the group milestones of each group at or under the owner above a
  member the view holds, once a group whatever the number of members,
  with the group's issue listing and issue statistics. Everything read
  from a group is seen as private: its milestones, their counts, their
  issues and why it was not read. A group milestone's title, counts and
  issues take every project of the group, including projects the source
  does not list: excluded by a filter, archived, or hidden from the
  token's listing. A public group may hold private projects, so neither
  the group's own visibility nor the listed members can show that a
  public audience may see them. Each of its issues is named by the
  project its reference names, and dropped where the view does not hold
  that project.
- Where two reads ask for one repository and see it apart, it is seen
  as the strictest of them. A one-repository source's own repository is
  seen as private only where the release picks none of its pull requests;
  where it does, it is seen as those pull requests are.
- The header reads at most 20 repositories and groups a round, across
  every source of the view. They are ordered public, then internal, then
  private; within each, those holding a pull request of the release
  first, then by path. A repository or group past the 20 is named unread
  with its reason, seen as its own visibility.
- Every read goes through the forge's rate-limit, refusal and answer
  checks, as the reads of ADR-0082 and ADR-0084 do, and a read the
  forge or the run's budget refused is named unread with why.
- Every request is made on the source's own API, so a self-managed
  GitLab or a GitHub Enterprise Server owner spans its members the same
  way.

## Consequences

- A public audience is told the same header whether or not an owner has
  private or internal members: the reads it may see are made first, and
  each item it may not see is dropped before it is shown, whatever
  `release.withheld` says of the verdict.
- Group milestones serve private routes alone. A public route still sees
  each public member's own milestones and issues.
- A view of a large owner makes up to 20 more requests a round, plus the
  milestone and issue requests each read already makes; doctor names the
  cost.
- A release held to one repository reads nothing of the rest, so its
  verdict, header and alerts (ADR-0085) are that repository's alone.
