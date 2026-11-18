# ADR-0051: A forge source announces its security alerts to private places

**Status:** Accepted
**Date:** 2026-11-18

## Context

Forges raise security alerts: GitHub's code scanning, Dependabot and
secret scanning alerts, and GitLab Ultimate's vulnerabilities. A
maintainer learns of them by mail or by visiting each repository. An
alert names a weakness that may not be public yet, so where it is posted
matters more than for any kind herald announced before. A route's
`private` said only whether a private repository's events may reach it
(ADR-0029), and an app may post any kind (ADR-0007).

## Decision

1. **`alert` is a signal and a private kind.** `signals = ["alert"]`, per
   source and per member override, announces `alert` events (ADR-0018),
   whose attributes are `action`, `tool`, `severity`, `rule`, `package`,
   `ecosystem`, `path`, `number` and `resolution`, and whose `alert` link
   is the alert's page. A kind may be private: `alert` ships private, and
   `[kinds.<name>] private = true` marks a declared kind.
2. **A route taking a private kind says where it goes.** A route whose
   kinds, named or taken from its sources and apps, hold a private kind
   states `private = true`, or `private = false` to post to a public place
   on purpose; a route fed by an app counts every kind the app may post.
   Load refuses a silent route; `[doctor] private_kinds = "warn"` lets it
   run and doctor names it. At run time a private kind's event, or one of
   a kind herald does not know, skips a silent route while
   `private_kinds` refuses, so a route the load check missed still fails
   closed.
3. **One vocabulary on every forge.** `tool` is what raised the alert:
   `code-scanning` (GitHub's code scanning, GitLab's SAST),
   `dependency-scanning` (Dependabot, GitLab's dependency scanning),
   `secret-scanning` (secret scanning, GitLab's secret detection), and on
   GitLab `container-scanning`, `dast`, `fuzzing` and `other`. `severity`
   is lowercase: a code scanning alert's security severity, else its rule's
   `error`, `warning` or `note`; Dependabot's `moderate` reads as `medium`.
4. **What an alert did.** `created`, `reopened`, `fixed`, `dismissed`,
   and GitLab's `confirmed`. A secret scanning alert resolved as `revoked`
   reads as `fixed` and any other resolution as `dismissed`, with
   `resolution` kept. A GitLab vulnerability `resolved` reads as `fixed`.
5. **`alerts` narrows per source.** `tools` (every tool the forge raises
   where absent), `actions` (`created` and `reopened` where absent) and
   `severities` (every alert where empty). Forgejo, which raises no
   alerts, is refused, as are a tool the forge lacks, `confirmed` beside
   GitHub, an empty `tools` or `actions`, and `alerts` without `alert` in
   the signals.
6. **An alert's identity shares its first sighting.**
   `<host>/<repository id>/alert/<tool>/<number>/created` for the alert
   raised, which a webhook and a poll name alike, and
   `.../<action>/<unix seconds of the change>` for what it did after.
7. **Webhooks hear everything; polls hear sightings and closings.**
   GitHub's `code_scanning_alert`, `dependabot_alert` and
   `secret_scanning_alert`, and GitLab's `Vulnerability Hook` announce at
   once. GitLab's hook names no project path, so it is read from the
   vulnerability's page under the instance's web root, a relative root
   included; a page elsewhere is refused. A poll reads each GitHub tool's
   alerts, most recently updated first, under the tool's own validators,
   and GitLab's newest detected vulnerabilities through GraphQL. A poll
   announces the alerts it lists first seen and closed; a reopening, and
   the closing of an alert older than the listing, comes only by webhook.
   GitLab marks no reopening, so a vulnerability set back to detected is
   not announced again. An owner's events name no alerts, so an owner
   source announcing alerts polls every member each round, and a GitLab
   member hook herald registers subscribes to vulnerabilities. GitHub
   Actions runs on no alert event, so alerts have no CI path.
8. **An alert's event is built from an allowlist.** Only the attributes
   above are read; a secret's value is never parsed.
9. **Doctor names what a source cannot hear.** GitHub's alerts need the
   tool enabled and a token or App that may read them; GitLab's need
   Ultimate; a source without a webhook misses reopenings and late
   closings; an owner source announcing alerts polls every member.

## Consequences

- Security alerts reach a maintainers' channel from `herald.toml`, and
  never a public one by accident: a route has to say so.
- Every route fed by an app that names no `kinds` now states `private`,
  since an app may post an alert.
- Polling alerts costs one conditional request per GitHub tool, and one
  GraphQL request on GitLab, per repository each round.
