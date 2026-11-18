# ADR-0053: A forge source announces its issues

**Status:** Accepted
**Date:** 2026-11-18

## Context

Issues are the other half of a maintainer's inbox beside pull requests
(ADR-0052), and follow the same pattern: a signal per source, one
vocabulary on every forge, and a route matching by attributes.

## Decision

1. **`issue` is a signal and a kind.** `signals = ["issue"]` announces
   GitHub's, GitLab's and Forgejo's issues, whose attributes are `action`,
   `number`, `author`, `bot`, `state_reason` (GitHub's, on a closing) and
   `labels`, and whose `issue` link is its page. It is not private; the
   repository's visibility bounds it (ADR-0029).
2. **What it did:** `opened`, `closed`, `reopened`, every one where
   `issues.actions` is absent. `issues` also narrows by `labels` and
   `authors` as globs.
3. **Identity** is `<host>/<repository id>/issue/<number>/opened`, shared
   by a webhook and a poll, and `.../<action>/<unix seconds>` after.
4. **Paths.** Webhooks (`issues`, GitLab's Issue Hook) announce at once. A
   poll reads the newest issues under its validators and announces
   openings and closings; GitHub's listing names pull requests too, which
   are left out. A reopening comes only by webhook. GitLab's webhook names
   only who acted, so its author is known from the opening alone. An owner
   source announcing issues polls every member each round, and a GitLab
   member hook herald registers subscribes to issues.

5. **Confidential issues fail closed.** GitLab's confidential issues are
   left out unless `issues.confidential = true` (GitLab only): a poll asks
   for `confidential=false` and drops any it lists anyway, a webhook's is
   dropped, and a Confidential Issue Hook delivery is confidential whatever
   its payload says. With it set, each is announced seen as private
   (ADR-0035 decision 11), so only a route declared private takes it while
   `[doctor] private_to_public` refuses, and a member hook subscribes to
   confidential issues. A later bound, such as the visibility of the
   member a held or delivered event is of, only ever tightens who may see
   an event, so a public repository never loosens it.

## Consequences

- New and closed issues reach chat from `herald.toml`.
- Polling issues costs one conditional request per repository each round.
