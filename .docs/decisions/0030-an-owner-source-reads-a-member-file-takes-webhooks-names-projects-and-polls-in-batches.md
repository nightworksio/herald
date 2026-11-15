# ADR-0030: An owner source reads a member file, takes webhooks, names projects and polls in batches

**Status:** Accepted
**Date:** 2026-11-15

## Context

An owner source lists an owner's repositories and polls each one it takes
on its own (ADR-0029). Some organizations keep the list of repositories
they ship in a file, such as lemonfiber's `spec/30-repos/repos.toml`, and
want that file to decide membership. A forge pushes a release to herald
faster than any poll finds it, and an organization webhook covers every
repository at once. A member announced under its repository's name alone
can collide with another owner's, or read poorly in a channel. Polling
each member with its own REST request costs one request per member every
round, while GitHub's GraphQL API answers many repositories in one
request.

## Decision

1. **A member file names the members**, and the filters narrow it.
   `members = { file = "repos.toml" }` is read beside the config when it
   loads; `members = { repository, path, ref }` is read through the forge's
   API with the source's token each round, asked with the `ETag` the last
   listing kept. `tables` names the arrays that list repositories and `key`
   the field naming each one ("name" where absent), in TOML or JSON by the
   file's extension. An entry matches a repository's name or full path,
   ignoring case. A file read anew reads the listing whole, and a listing
   that changed while the file did not reads the file again. A file that
   cannot be read fails the listing, which keeps the last membership
   (ADR-0029). `herald doctor --online` names as a mistake each entry the
   owner does not hold.
2. **An owner source takes webhooks** from its repositories and from its
   organization. A delivery of a repository the owner does not hold is
   refused. One the filters do not take, by the attributes its payload
   names, announces nothing; an attribute the payload leaves out is not
   filtered on. A delivery is announced only where the owner's last
   membership record holds the repository, and has not left, when its work
   runs; one whose record cannot be read releases its delivery so the forge
   delivers it again. Its project is named from the membership record. Its
   routes are bounded as a polled member's are, by the stricter of the
   visibility the delivery names and the one the record holds: it counts as
   public only where both say so, and a delivery naming none counts as
   private, so a repository made private since the last listing reaches
   only `private` routes. A webhook-only owner source announces nothing until its polls
   have listed the owner, and doctor says so.
3. **A `project` template names a member's project**, from
   `repository.id`, `name`, `path`, `owner`, `topics`, `properties` and
   `visibility`. A template reading any other name does not load. A
   member it names no project for is reported each round and not read.
   `[sources.<owner>.repositories."<glob>"]` overrides it per repository,
   settled key by key: the narrowest glob setting a key gives it, a glob
   with no wildcard before one with, and otherwise the one naming more
   characters as themselves. Two globs as narrow as each other that set
   one key and could both name a repository do not load.
4. **No member announces as another's project.** A repository's admin
   controls its `name`, `topics`, `properties` and `visibility` (and on
   GitLab its `path`), so a template reading them can name any project.
   Each round lists every owner first, then checks each member's project,
   templated or not, against every other member of every owner, every
   source's and app's configured project, and the project each source's
   cursor last announced as. A member whose project another announces as
   is withheld, polled and announced from no webhook, and reported naming
   the other, until they differ; a configured source is never withheld.
   Where any source's records cannot be read, every member is withheld
   that round, as its project cannot be checked.
5. **A GitHub owner polls its members in GraphQL batches** under
   `transport = "auto"` where the source has a token, and with
   `transport = "graphql"`; `"rest"` polls each member with its own REST
   request. A batch of `graphql_batch` members (50, at most 100) is two
   requests at most: the first asks each member's newest releases by tag,
   and the second asks in full only the members holding one their cursor
   has not seen. A member GitHub names no repository for fails alone.
   The GraphQL endpoint is `graphql`, or the one beside `api`:
   `/api/graphql` on a GitHub Enterprise Server whose `api` ends in
   `/api/v3`, and `/graphql` on GitHub and GHE.com. It must be on `api`'s
   origin, the only one the source reaches (ADR-0007).
6. **`transport = "events"` follows the owner's events.** The listing
   still runs each round. The owner's events are read once a round,
   conditionally on their `ETag`, page by page until one reaches back to
   the newest event the last read saw; only members never polled and
   members a published `ReleaseEvent` names are polled, through GraphQL or
   REST. Where the events do not reach back, GitHub keeps no more of them,
   there was no last read or the read fails, every member is polled that
   round. The listing record keeps how far the events were read.
7. **One member's membership is read alone** through the `Cursors` port,
   so a webhook's work reads one record, not the owner's every member.

```toml
[sources.org]
kind = "github"
owner = "lemonfiber"
token = { secret = "GITHUB_TOKEN" }
webhook = { secret = "HERALD_WEBHOOK" }
members = { repository = "lemonfiber/spec", path = "30-repos/repos.toml", ref = "main" }
tables = ["repo", "ungoverned"]
project = "{{ repository.name }}"

[sources.org.repositories."plugin-*"]
project = "plugins/{{ repository.name }}"
```

## Consequences

**The member file stays the one list an organization keeps**: herald
follows it without its own copy.

**A release reaches its routes as soon as the forge delivers it**, and a
repository that left the owner or its filters announces nothing from a
delivery.

**Polling an unchanged GitHub owner's members costs one request per 50
members a round** under GraphQL, where REST costs one per member, and one
conditional request a round under events.

**A repository admin cannot announce under another's name**: a project
that two would share is announced by neither member until it is settled.
