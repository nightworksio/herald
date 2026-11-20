# ADR-0062: A view reads an owner's repositories by one query over them

**Status:** Accepted
**Date:** 2026-11-20

## Context

A view read only sources naming one repository (ADR-0055), so an
organisation's queue needed a source per repository. An owner source
already knows its repositories: its membership record holds each one its
selection takes, who may see it and the project it is announced as
(ADR-0029, ADR-0030). Reading each member's listing costs a request per
repository a round; each forge has one query over an owner's open pull
requests instead. Separately, a live read through a REST listing without
a token takes every row as private, though the listing states the
repository's visibility, which some public projects would rather show.

## Decision

1. **A view reads an owner source.** A source naming an `owner` lists
   pull requests a view reads, and the built-in view reads it too.
2. **One query over the owner.** GitHub's issue search for
   `is:pr is:open org:<owner>` (`user:` for a user), GitLab's group merge
   request listing with its subgroups, and Forgejo's issue search by
   owner. A query's rows are kept only where the owner's membership
   record holds their repository and it has not left; each is named and
   seen as that record says. Where GitHub's search finds more than the
   1,000 it lists, or the owner is a GitLab user, each repository is
   listed on its own. A view reads an owner only once a poll listed its
   repositories, and is stopped, saying so, before then.
3. **Reading in full across repositories.** With a token, GitHub reads up
   to 100 of the pull requests in one GraphQL request across repositories,
   each seen as the stricter of what its repository's answer and its
   membership record say. Without, and on Forgejo, each pull request is
   read first where its listing named no head, then its reviews and its
   head's checks, at most `most_pulls` a round. GitLab's listing reads
   them in full.
4. **Kept mode by member.** A kept owner keeps one record per pull request
   as a repository does (ADR-0061), its reconcile mark holding each member
   repository the query or a delivery named. The query is asked
   conditionally where the forge answers with validators. A delivery is
   folded only for a member the membership record holds and has not left,
   named as the member's project; a delivery never widens a record. A
   repository that left has its records closed and is forgotten. Each
   record is seen at least as strictly as its membership record says now.
   A record whose listing named no head is stale only where its last
   activity or repository changed.
5. **A kept record closes only on a whole listing.** A kept open record
   no longer listed is closed only where the listing held fewer than
   `most_open`, for one repository and an owner alike.
6. **Visibility a listing states.** `[views.<name>] listed_visibility`,
   off by default, makes a live read through a REST listing without a
   token take the repository's visibility from the listing's `base.repo`
   (`visibility`, else `private` and `internal`) on GitHub, GitHub
   Enterprise Server, GHE.com and Forgejo, self-hosted included. It widens
   nothing else: not a read with a token, not a GitLab listing, which
   states none, not a kept record (ADR-0061 decision 5), and not an
   owner's rows.
7. **Doctor** names each owner a report's view reads, by which query and
   at what cost, and each view that sets `listed_visibility`.

## Consequences

- Every request a view's read makes, its search, each member's listing
  where the search falls back, its reconcile and each read in full, is
  charged to the run's budget as it is made: a read stops at the host's
  limit with the budget's reason and makes no request past it.
- An organisation's queue costs a search page per 100 open pull requests
  and a GraphQL request per 100 read in full, with a token on GitHub.
- GitHub's search is counted against its own rate limit, 30 searches a
  minute with a token.
- An anonymous read can see only public repositories, so
  `listed_visibility` widens rows only where a self-hosted forge or a
  proxy in front of it serves more to anonymous readers than the public.
