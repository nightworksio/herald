# Runs

A forge source whose `signals` include `"run"` announces the repository's
CI runs as `run` events. An owner source announces its members' runs, and
a member override's `signals` chooses them per repository.

```toml
[sources.cli]
kind = "github"
repository = "nightworksio/herald"
signals = ["release", "run"]
runs = { conclusions = ["failure", "timed_out", "startup_failure"], branches = ["main"] }
```

## What a run is read from

| Forge | `runs.from = ["actions"]` (the default) | `runs.from = ["checks"]` |
|-------|------------------------------------------|---------------------------|
| GitHub, GitHub Enterprise Server, GHE.com | GitHub Actions' workflow runs | check suites of every app but GitHub Actions, and commit statuses |
| GitLab, self-managed GitLab | pipelines | refused |
| Forgejo, Gitea | Forgejo Actions' runs | refused |

## The keys

| Key | What it chooses | Where absent |
|-----|-----------------|--------------|
| `runs.from` | `actions`, `checks` | `["actions"]` |
| `runs.actions` | what a run did: `requested`, `in_progress`, `completed` | `["completed"]` |
| `runs.conclusions` | how a finished run ended: `success`, `failure`, `cancelled`, `timed_out`, `skipped`, `action_required`, `neutral`, `startup_failure`, `stale` | every conclusion |
| `runs.branches` | the branches or tags, as globs | every branch |
| `runs.triggers` | what started the run, such as `push`, `pull_request` or `schedule` | every trigger |
| `runs.workflows` | the workflow, pipeline, check suite's app or status context, as globs | every workflow |

Where one of `actions`, `conclusions`, `triggers` or `branches` names a
single value (a branch with no glob in it), a poll asks the forge for only
those runs. Every key is applied again to whatever the forge answers.

## The event

A run's title says what it did, as in `CI failed on main`, its body is
the commit's title where the forge names one, and its `run` link opens the
run. Its attributes:

| Attribute | Holds |
|-----------|-------|
| `signal` | `run` |
| `action` | `requested`, `in_progress` or `completed` |
| `conclusion` | how a finished run ended, in GitHub's words |
| `workflow` | the workflow's name, the pipeline's name or `pipeline`, Forgejo's workflow file, the check suite's app or the status context |
| `trigger` | what started it; GitLab's `merge_request_event` reads as `pull_request` |
| `branch`, `commit` | the branch or tag and the commit it ran on |
| `number`, `attempt` | the forge's run number and attempt, where it names them |
| `actor` | who started it |
| `from` | `actions` or `checks` |

GitLab's statuses read as GitHub's conclusions: `failed` is `failure`,
`canceled` is `cancelled` and `manual` is `action_required`. A commit
status's `error` is `failure`, and a pending status is `in_progress`.

A run's identity is `<forge host>/<repository id>/run/<run id>/<attempt>`,
with `check-suite` or `status` in place of `run` for checks. GitLab,
Forgejo and checks name no attempt, so the run's conclusion takes its
place, and a run run again announces again only where it ends otherwise.
An unfinished run's identity also ends with what it did.

## Routing runs

A route's `match.attributes` takes runs by any attribute, and `match.not`
leaves some out. Every run on main, and pull requests' runs only where
they fail, to one channel:

```toml
[[routes]]
id = "build-log"
sources = ["cli"]
kinds = ["run"]
destination = "discord"
channel = "1566184281406177531"
template = "run"
match = { not = { attributes = { trigger = ["pull_request"] } } }

[[routes]]
id = "red-pull-requests"
sources = ["cli"]
kinds = ["run"]
destination = "discord"
channel = "1566184281406177531"
template = "run"
match = { attributes = { trigger = ["pull_request"], conclusion = ["failure", "timed_out", "startup_failure"] } }
```

## Webhooks and polls

| Forge | Webhook events | Polled |
|-------|----------------|--------|
| GitHub | `workflow_run`, and for checks `check_suite` and `status` | `GET /repos/{owner}/{repo}/actions/runs`; checks read the check suites and statuses of one commit, the newest of the one branch `runs.branches` names, or of the default branch |
| GitLab | Pipeline events (`Pipeline Hook`); a member hook herald registers subscribes to them where the member announces runs | `GET /projects/:id/pipelines`, newest updated first |
| Forgejo | `workflow_run_*`, and `action_run_*` on older releases | `GET /repos/{owner}/{repo}/actions/runs` |

Each listing keeps its own validators, so an unchanged one costs a
conditional request. An owner's events and Forgejo's activity feed name
no runs, so an owner source announcing runs polls every member each round
whatever its `transport`; a webhook or a GitHub App delivers runs as they
happen. A GitHub token or App reads workflow runs with Actions read
access, check suites with Checks read access, and statuses with Commit
statuses read access.

A GitHub Actions workflow started by `workflow_run`, `check_suite` or
`status` announces its run with `herald announce --from github`:

```yaml
on:
  workflow_run:
    workflows: ["CI"]
    types: [completed]
```
