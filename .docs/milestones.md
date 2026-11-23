# Milestones

A forge source whose `signals` include `"milestone"` announces the
repository's milestones as `milestone` events (ADR-0079). An owner source
announces its members', and a member override's `signals` chooses them per
repository.

```toml
[sources.cli]
kind = "gitlab"
repository = "group/web"
signals = ["release", "milestone"]
milestones = { actions = ["closed"], titles = ["v*"], group = true }
```

## The keys

| Key | What it chooses | Where absent |
|-----|-----------------|--------------|
| `milestones.actions` | what a milestone did: `opened`, `closed`, `reopened`, `deleted` | `closed` |
| `milestones.titles` | the titles, as globs | every milestone |
| `milestones.counts` | whether it names how many of its issues are open and closed | `true` |
| `milestones.group` | GitLab only: whether the milestones of the group holding the project are read too | `false` |
| `milestones.most` | the most open milestones a poll follows, at most 1000 | 100 |

## The event

A milestone is versioned by its title, so a route's `versions` and
`prereleases` take it as they take a Release. Its title says what it did,
as in `Milestone v1.4 closed: 12 of 14 closed, due 2026-12-01`, its body is
its description cut to 4 KiB, and its `milestone` link opens it.

| Attribute | Holds |
|-----------|-------|
| `signal` | `milestone` |
| `action` | what it did |
| `number` | GitHub's number, GitLab's iid, Forgejo's id |
| `milestone` | its title, which is also its `version` |
| `due` | the day it is due, as `YYYY-MM-DD` |
| `issues_open`, `issues_closed` | its forge's counts, where read |

Its identity is `<host>/<repository id>/milestone/<id>/opened` for its
opening and `.../<action>/<unix seconds>` for what it did after; a GitLab
group's milestone is named `<host>/group/<group id>/milestone/...`. A
deletion takes the identity of the opening or closing it withdraws from
each route with `withdraw = "delete"`.

## How milestones reach herald

| Path | GitHub | GitLab | Forgejo |
|------|--------|--------|---------|
| Webhook | `milestone` | Milestone Hook (`milestone_events`), from GitLab 18.2 | none |
| Poll | `/milestones?state=open` | `/milestones?state=active`, and the group's | `/milestones?state=open` |
| Counts | in the milestone, issues and pull requests | `/issues_statistics`, issues only, one request per announced milestone | in the milestone |

A poll asks each listing of open milestones under its validators and keeps
their ids. A milestone that appears is announced as opened, or reopened
where herald saw it close; one that leaves the listing is read once and
announced where it closed. The first poll only follows. A deletion comes
only by webhook. A private or internal repository's milestones reach only
private routes while `[doctor] private_to_public` refuses.

`announce --from github` reads a run's own `milestone` event too.
