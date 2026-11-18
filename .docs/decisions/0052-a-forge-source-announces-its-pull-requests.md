# ADR-0052: A forge source announces its pull requests

**Status:** Accepted
**Date:** 2026-11-18

## Context

A maintainer follows pull requests, bots' above all, by mail or by
visiting each repository. The maintainer dashboard (step 13d) acts on bot
pull requests across repositories, and needs to tell them apart. The kinds
`run` (ADR-0050) and `alert` (ADR-0051) set the pattern: a signal per
source, one vocabulary on every forge, and a route matching by attributes.

## Decision

1. **`pull-request` is a signal and a kind.** `signals = ["pull-request"]`
   announces GitHub's and Forgejo's pull requests and GitLab's merge
   requests, whose attributes are `action`, `number`, `author`, `bot`,
   `base`, `head`, `draft`, `merged_by` and `labels` (joined by a comma
   and a space), and whose `pull` link is its page. It carries the
   lifecycle only; reviews and checks are kinds of their own.
2. **What it did.** `opened`, `ready_for_review`, `reopened`, `merged`, and
   `closed` for one closed unmerged; `opened`, `ready_for_review` and
   `merged` where `pulls.actions` is absent. GitLab's `update` reads as
   `ready_for_review` where it moves a draft out of draft.
3. **Drafts wait.** `pulls.drafts` is false by default: a draft's opening
   is not announced, its readiness is.
4. **`pulls` narrows per source:** `actions`, `drafts`, and `labels` and
   `authors` as globs.
5. **Bots are marked.** `bot` holds GitHub's `Bot` account type, a login
   ending in `[bot]`, or on GitLab a project or group access token's
   username or one ending in `-bot`.
6. **Identity** is `<host>/<repository id>/pull/<number>/opened`, shared by
   a webhook and a poll, and `.../<action>/<unix seconds>` after.
7. **Paths.** Webhooks (`pull_request`, GitLab's Merge Request Hook)
   announce at once. A poll reads the newest pull requests under its
   validators and announces openings, merges and closes; readiness and
   reopenings come only by webhook. GitLab's webhook names only who acted,
   so its author is known from the opening alone. An owner source
   announcing pull requests polls every member each round, and a GitLab
   member hook herald registers subscribes to merge requests.
8. **Not private.** A repository's visibility bounds its pull requests as
   it bounds every event (ADR-0029).

## Consequences

- Bot pull requests, merges and closes reach chat from `herald.toml`, and
  the dashboard reads the same events.
- Polling pull requests costs one conditional request per repository each
  round.
