# ADR-0066: A view shows its repositories' CI capacity

**Status:** Accepted
**Date:** 2026-11-20

## Context

A queue of open pull requests says whose turn each is, but not whether
the CI they wait on is saturated, nor which runs are wasted on commits a
newer push replaced. The user decided that overviews are sections of a
view, and that every choice of which repositories they read is a setting
with the recommended one the default.

## Decision

1. **A section of a view.** `[views.<name>] overviews = ["capacity"]`
   shows, beside its rows, the runs queued and running in its
   repositories, the oldest queued, and those superseded: started for a
   pull request on a commit no open pull request's head is. Templates
   read it as `view.overviews`.
2. **Which repositories.** `capacity_from = "shown"`, the default, reads
   the repositories the view shows a pull request of. `"members"` reads
   the owner's newest pushed members the membership record holds, at
   most `capacity_repositories` (20, from 1 to 100), ordered by one
   request: GitHub's `sort=pushed`, GitLab's `order_by=last_activity_at`.
   Forgejo lists no push order, so it takes the first its membership
   record keeps. A view with a `repositories` filter takes the first
   members it holds by name, with no request for their order, so the
   push times of the repositories it hides choose nothing (ADR-0067).
3. **What each forge is asked.** GitHub Actions' runs with
   `status=queued` and `status=in_progress`; GitLab's pipelines with
   `status=pending` and `status=running`; Forgejo's Actions tasks,
   those waiting and running. Runs started by a pull request or merge
   request event are a pull request's.
4. **Only what the view holds.** The repositories a capacity section
   reads, and every run and unread entry it shows, are those the view's
   own `repositories` filter holds: "shown" takes the repositories of the
   view's rows, after its filters; "members" takes only the members the
   filter holds, before anything is read. One function of the view
   decides it, the same its rows take, and every overview is narrowed by
   it before it is shown, so nothing of another repository is read or
   named. An unread entry naming a source or an owner, a read that failed
   as a whole, is always kept, so a failed read never shows as clean.
5. **Unread is never idle.** A repository whose runs the forge refuses
   (401, 403) or does not find (404), and a Forgejo with no tasks API, is
   named in `view.overviews.unread` with the permission it needs or the
   reason, and the section shows empty beside it, never absent. A read
   that fails otherwise names the source unread with what stopped it. A
   listing that fills its page (100, or 50 on Forgejo) is named as
   holding more than it counts.
6. **Every request is charged.** The reads go through the run's budgeted
   handle, so no request is made past a host's request limit; the budget
   ending leaves the section unread with that reason, never the whole
   report stopped.
7. **Who sees which runs.** A run and an unread repository are seen as
   the rows of their repository are, else as the membership record holds
   it, else as private. Each audience of a report is shown only those it
   may see.

## Consequences

- A capacity section costs two requests a repository on GitHub and GitLab
  and one on Forgejo, and `"members"` one more; doctor names it, what is
  not counted, and the permission each forge's token needs.
