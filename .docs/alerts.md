# Alerts

A forge source whose `signals` include `"alert"` announces the
repository's security alerts as `alert` events: GitHub's code scanning,
Dependabot and secret scanning alerts, and GitLab Ultimate's
vulnerabilities. An owner source announces its members' alerts, and a
member override's `signals` chooses them per repository.

```toml
[sources.cli]
kind = "github"
repository = "nightworksio/herald"
signals = ["release", "alert"]
alerts = { severities = ["critical", "high"] }

[[routes]]
id = "security"
sources = ["cli"]
kinds = ["alert"]
destination = "discord"
channel = "1566200000000000500"
private = true
```

## Alerts belong in private places

`alert` is a private kind. A route that takes it, by its `kinds` or
because a source or app it names may announce it, says where it goes:
`private = true` where its place is private, or `private = false` to post
alerts to a public place on purpose. A route that says neither is refused
at load; `[doctor] private_kinds = "warn"` lets it run, and doctor names
it. While `private_kinds` refuses, an alert, or an event of a kind herald
does not know, never reaches a route that says neither. A kind declared
under `[kinds.<name>]` is private with `private = true`.

## What raises alerts

| `tool` | GitHub, GitHub Enterprise Server, GHE.com | GitLab, self-managed GitLab (Ultimate) |
|--------|-------------------------------------------|----------------------------------------|
| `code-scanning` | code scanning, such as CodeQL | SAST |
| `dependency-scanning` | Dependabot | dependency scanning |
| `secret-scanning` | secret scanning | secret detection |
| `container-scanning` | none | container and cluster image scanning |
| `dast` | none | DAST |
| `fuzzing` | none | coverage-guided and API fuzzing |
| `other` | none | any other scanner's report |

Forgejo and Gitea raise no alerts, and a source of theirs with `"alert"` in
its signals is refused.

## The keys

| Key | What it chooses | Where absent |
|-----|-----------------|--------------|
| `alerts.tools` | what raised the alert, from the table above | every tool the forge raises |
| `alerts.actions` | what the alert did: `created`, `reopened`, `confirmed` (GitLab), `fixed`, `dismissed` | `["created", "reopened"]` |
| `alerts.severities` | `critical`, `high`, `medium`, `low`, `info`, `unknown`, `error`, `warning`, `note` | every alert, those with no severity too |

## The event

An alert's title says what raised it and what it did, as in
`Dependency scanning alert #4 opened: Prototype pollution in left-pad`,
its body is empty, and its `alert` link opens the alert. Its attributes:

| Attribute | Holds |
|-----------|-------|
| `signal` | `alert` |
| `action` | what it did |
| `tool` | what raised it |
| `number` | its number in the repository |
| `severity` | a code scanning alert's security severity, else its rule's; Dependabot's `moderate` reads as `medium` |
| `rule` | the code scanning rule, the advisory's GHSA id, the secret's type, or the vulnerability's primary identifier |
| `package`, `ecosystem` | the dependency, where one is named |
| `path` | the file, manifest or container image |
| `resolution` | how a secret scanning alert was resolved; `revoked` reads as `fixed`, anything else as `dismissed` |

Nothing else is read from an alert: a secret's value is never parsed.

Its identity is `<host>/<repository id>/alert/<tool>/<number>/created`
where it was raised, so a webhook and a poll announce it once, and
`.../<action>/<unix seconds>` for what it did after.

## How alerts reach herald

| Path | GitHub | GitLab |
|------|--------|--------|
| Webhook | `code_scanning_alert`, `dependabot_alert`, `secret_scanning_alert` | Vulnerability Hook (`vulnerability_events`) |
| Poll | each tool's alerts, most recently updated first, under its own validators | the newest detected vulnerabilities, through GraphQL |

A poll announces the alerts it lists first seen and closed. A reopening,
and the closing of an alert older than the listing, comes only by webhook;
doctor says so where the source names none. GitLab marks no reopening: a
vulnerability set back to detected reads as its first sighting, which was
announced already.

GitLab's Vulnerability Hook names no project path; herald reads it from
the vulnerability's page under the instance's web root, `api` without its
`/api/v4`, so a self-managed instance under a relative root works too. A
member hook herald registers for an owner source subscribes to
vulnerabilities where the member takes alerts.

An owner's events and feed name no alerts, so an owner source announcing
alerts polls every member each round. GitHub Actions runs on no alert
event, so `announce --from github` has no alerts to read.

## What the forge needs

- GitHub lists a tool's alerts where the repository has it enabled, to a
  token with `security_events` (and `repo` for private repositories) or a
  GitHub App with read access to code scanning, Dependabot and secret
  scanning alerts. Narrow `alerts.tools` to the tools a repository has; a
  poll of one it lacks fails.
- GitLab raises vulnerabilities on GitLab Ultimate, to a token whose user
  may read them.

Doctor names each of these for a source announcing alerts.
