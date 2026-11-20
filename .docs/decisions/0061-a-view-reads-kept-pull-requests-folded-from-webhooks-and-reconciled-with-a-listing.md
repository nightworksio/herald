# ADR-0061: A view reads kept pull requests, folded from webhooks and reconciled with a listing

**Status:** Accepted
**Date:** 2026-11-19

## Context

A view reads its sources' open pull requests from the forge each time it
is shown (ADR-0055). That costs a round of requests per view and source,
knows only what the forge says now, and cannot tell when a pull request's
state began. Pages, metrics and commands at scale need what herald keeps
instead: a record per pull request, kept current by the forge's webhooks,
with the time each part of its state took its value. Webhooks are missed
and arrive out of order, so the records must be corrected from the forge
regularly, at little cost where nothing changed.

## Decision

1. **A view says where it reads.** `[views.<name>] mode` is `auto` (the
   default), `live` or `kept`. `auto` reads a source kept where the store
   keeps pull requests and the source takes webhooks, its own or its
   GitHub App's, and live otherwise. A kept view on a store that keeps no
   pull requests is a mistake doctor names, and stops the view.
2. **One record per pull request.** `Pulls` is a new store port, kept by
   every record store, faked by the test stores and checked by the store
   conformance suite with ten faults; the markers store keeps none. A
   record is keyed `<host>/<repository id>/<number>`, the host being that
   of the pull request's pages. It holds what a view reads, when its draft
   state, review, checks, mergeability, merge queue state and head last
   changed, each reviewer's latest verdict and each check's latest outcome
   on the head delivered since the last full read, and how it closed: when,
   and whether merged where known.
3. **Every delivery folds.** Pull request, review, review request, check,
   commit status, merge queue and comment deliveries of GitHub, GitLab and
   Forgejo are folded into the record of each pull request they are of; a
   check into each open record whose head it ran on. A delivery older than
   the record leaves it as it is; a comment moves only the last activity.
   Folded reviews count as GitHub's review decision does: a deciding
   change request outweighs an approval, a verdict of unknown weight reads
   unknown, and an approval cannot clear a change request the last read
   holds, so it reads unknown until the next read. A delivery the source
   also announces is left to both jobs; one that cannot be kept is
   released, so its sender delivers it again.
4. **A conditional reconcile.** Each round, or at most once each
   `[views.<name>] reconcile`, a kept source's open pull requests are
   listed, each page conditional on its last answer's validators. Where a
   page changed, the pull requests herald keeps no record of, whose head or
   last activity differs, which closed, or that were never read in full,
   are read in full; a record open and no longer listed is closed. A record
   whose checks still run is read in full on every listing, changed or
   not. Reading in full is one GraphQL request for up to 100 pull requests
   on GitHub with a token, the listing itself on GitLab, and each one's
   reviews and checks elsewhere, at most `most_pulls` a round; the rest
   are kept unread and read on a later round. Where each source's
   reconcile stands, its repository and its validators are kept in the
   same port.
5. **Who may see a kept pull request only narrows without a full read.**
   A record carries its repository's visibility, which bounds the routes
   its rows reach (ADR-0029, ADR-0055). A webhook delivery only ever
   narrows it, never widens it: a record a delivery begins is private
   until a full read, and GitHub's and Forgejo's `repository` deliveries
   narrow every record of the repository, open or closed. What a REST
   listing's `base.repo` names of the repository's visibility narrows
   every record of it, read that round or not, and never widens one; a
   listing's rows themselves read as private, as they do in live mode,
   which takes nothing from that field. Only a full read through GitHub's
   GraphQL widens who may see a record, so a repository gone private
   between two listings shows nothing of it as public, and one made
   public waits for its next full read. GitLab's listings and project
   webhooks name no project visibility, so its kept records stay private,
   as its live rows are.
6. **Closed pull requests age out.** `[store] keep_pulls` (30 days) keeps
   a closed pull request's record whole, for later metrics; housekeeping
   forgets older ones.
7. **Doctor** names each source a view reads kept and how often it is
   listed, and a kept source taking no webhooks.

## Consequences

- A kept view on GitHub costs one conditional request per page a round
  where nothing changed, which GitHub does not count against its rate
  limit.
- GitHub's `check_suite` deliveries are not folded: their check runs
  carry each outcome, and a repository's webhook is sent only `completed`.
- GitLab's merge request deliveries name their author only on opening,
  and Forgejo's reviews name no weight; the next full read settles both.
- The records' state times are what pages, metrics and service levels of
  later phases read.
