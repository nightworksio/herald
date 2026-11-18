# Issues

A forge source whose `signals` include `"issue"` announces the
repository's issues as `issue` events. An owner source announces its
members', and a member override's `signals` chooses them per repository.

```toml
[sources.cli]
kind = "gitlab"
repository = "group/web"
signals = ["release", "issue"]
issues = { actions = ["opened"], labels = ["bug", "security*"] }
```

## The keys

| Key | What it chooses | Where absent |
|-----|-----------------|--------------|
| `issues.actions` | what an issue did: `opened`, `closed`, `reopened` | every action |
| `issues.labels` | the labels, as globs, any one of which it carries | every issue |
| `issues.authors` | the authors, as globs | every author |
| `issues.confidential` | GitLab only: whether confidential issues are announced, each as a private event | `false` |

## Confidential issues

GitLab's confidential issues stay out unless `issues.confidential = true`:
a poll asks GitLab for `confidential=false` and still drops any
confidential issue it lists, and a webhook's confidential issue is dropped.
A delivery through the Confidential Issue Hook is confidential whatever its
payload says. With it
set, a confidential issue is announced as a private event, so it reaches
only a route declared `private = true` while `[doctor] private_to_public`
refuses, and a member hook herald registers subscribes to confidential
issues too. GitHub and Forgejo have no confidential issues; a private
repository's issues are bounded by its visibility like every event.

## The event

Its title says what it did, as in `Issue #9 closed: Crash on start`, and
its `issue` link opens it.

| Attribute | Holds |
|-----------|-------|
| `signal` | `issue` |
| `action` | what it did |
| `number` | its number in the repository |
| `author` | who opened it, where the forge names them |
| `bot` | whether its author is a bot or an app |
| `state_reason` | why a GitHub issue closed: `completed`, `not_planned` or `duplicate` |
| `labels` | its labels, joined by a comma and a space |

Its identity is `<host>/<repository id>/issue/<number>/opened` for its
opening, which a webhook and a poll name alike, and
`.../<action>/<unix seconds>` for what it did after.

## How issues reach herald

| Path | GitHub | GitLab | Forgejo |
|------|--------|--------|---------|
| Webhook | `issues` | Issue Hook (`issues_events`), Confidential Issue Hook | `issues` |
| Poll | `/issues`, most recently updated first, pull requests left out | `/issues`, most recently updated first | `/issues?type=issues`, most recently updated first |

A poll reads the listing under its validators and announces the openings
and closings among the newest it lists; a reopened issue comes only by
webhook, and doctor says so where the source names none. GitLab's webhook
names only who acted, so an issue's author is known from its opening
alone. An owner source announcing issues polls every member each round.
