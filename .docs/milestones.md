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
| `milestones.actions` | what a milestone did: `opened`, `closed`, `reopened`, `deleted`, `overdue` | `closed` |
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
opening, `.../overdue/<due day>` for its due day passing, and
`.../<action>/<unix seconds>` for what it did after; a GitLab
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

## Overdue

No forge sends a milestone overdue, so only a poll finds one, by its own
clock (ADR-0080). A poll keeps each open milestone's due day, and on its
first round after that day ends in UTC announces it once as `overdue`,
dated the start of the next day; a milestone whose due day moves is
announced again when the new day passes. A poll keeps which days have
passed whether or not `overdue` is chosen, so choosing it later announces
only the days that pass after. One whose listing did not change is read
once by its own path when its day passes. A source that is only delivered
webhooks and never polled never announces it, and doctor says so.

## Output

`herald poll` and `herald announce` name a private or internal
repository's event `(private)` in the lines saying what became of it,
since a CI job's log may be public.
`--private`, or `[cli] private = true`, says the output is private and
names it by its version, a milestone's title. A server's log says
`(private)` always.

`announce --from github` reads a run's own `milestone` event too.
