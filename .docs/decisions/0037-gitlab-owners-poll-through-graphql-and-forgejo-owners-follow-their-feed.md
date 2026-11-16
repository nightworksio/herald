# ADR-0037: GitLab owners poll through GraphQL, and Forgejo owners follow their feed

**Status:** Accepted
**Date:** 2026-11-16

## Context

A GitHub owner polls its members in GraphQL batches or follows its events
(ADR-0030). GitLab and Forgejo owners polled each member over REST, one
request per member per round.

## Decision

1. **GitLab offers `transport = "graphql"`.** Each batch of
   `graphql_batch` members is asked by project id in one request
   (`projects(ids:)`), first for each project's newest Releases by tag,
   then in full for those holding one not yet seen, as on GitHub. Its
   identities are those REST gives. `auto` uses it where the source has a
   token; `graphql` uses it without one too. The endpoint is `/api/graphql`
   beside `/api/v4`, or `graphql`.
2. **GitLab's GraphQL reads Releases only.** It lists no tags or packages,
   so an owner whose members take either polls over REST, and
   `transport = "graphql"` does not load there.
3. **Forgejo offers `transport = "feed"`.** The owner's activity feed
   (`/orgs/<owner>/activities/feeds`, or `/users/<owner>/...`) is read once
   a round, `feed_page` activities to a page (50 where absent, at most
   200) and at most `feed_pages` pages back (3, at most 12), and only
   the members it names publishing a Release, or pushing a tag where a
   member takes tags, are polled, with those never polled; a feed that
   does not reach back to the last read polls every member. REST per
   member stays `auto` and `rest`.
4. **Each forge names the transports it offers.** GitHub: auto, graphql,
   rest, events. GitLab: auto, graphql, rest. Forgejo: auto, rest, feed.
   Any other does not load, naming those offered.

## Consequences

- A GitLab group of N release-only projects costs one or two requests a
  round per batch rather than N.
- A Forgejo organization costs one feed request a round, plus a request
  per member it names.
