# ADR-0063: A view predicts conflicts from the paths open pull requests change

**Status:** Accepted
**Date:** 2026-11-20

## Context

The forge says whether a pull request conflicts with its branch now, but
not which open pull requests will conflict with each other once one
merges. Maintainers ordering a queue want to know which pull requests
touch the same files. Reading every pull request's changed files costs a
request each on most forges, so it must be asked for, and kept where
herald keeps records.

## Decision

1. **Opt-in per view.** `[views.<name>] conflicts`, off by default, makes
   a view read the paths each pull request's head changes, at most 100
   each, and the branch it merges into.
2. **Overlaps across every open pull request.** Each row's `overlaps`
   names, by number, the open pull requests of its repository merging
   into the same branch whose heads change a path in common, the view's
   hidden rows included. A pull request whose branch or paths were not
   read overlaps none. A renamed file counts its old path too. The
   forge's own conflict state keeps its place among herald's rules.
3. **Read where it is cheapest.** GitHub's GraphQL reads the paths in the
   same request as each pull request. It names no renamed file's old
   path, so a pull request it names a renamed file of has its first page
   of changed files read over REST, one request more, at most
   `most_pulls` a round; one past them reads as partial. Without a
   token, and on Forgejo, a pull request read in full takes one request
   more; on GitLab each merge request's diffs take one, at most
   `most_pulls` a round. A read naming
   every path says so, so a page cut at 100 is known as partial.
4. **Kept once a head.** A kept record keeps the paths until a delivery
   or a read moves its head. A record kept without them is read in full
   once a view predicting conflicts reads it.
5. **Doctor** adds what reading the paths costs to each report's
   requests a round.

## Consequences

- A view predicting conflicts on GitHub with a token costs a larger
  answer, and a request more only for each pull request renaming a file.
- Areas and CODEOWNERS (phase 4) can read the same paths.
