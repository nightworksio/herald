# ADR-0050: A forge source announces its CI runs, and a route matches by attributes

**Status:** Accepted
**Date:** 2026-11-17

## Context

herald announced releases, tags and packages. CI failures, nightly jobs
and a build log reached chat only through workflows that built an app
event by hand. Every forge reports runs: GitHub Actions' workflow runs,
GitLab's pipelines and Forgejo Actions' runs, and other CI services report
to GitHub as check suites and commit statuses. A run is routed by what it
is (how it ended, what started it), which a route's `match` could not read:
it narrowed by the origin alone (ADR-0038).

## Decision

1. **`run` is a signal and a kind.** `signals = ["run"]`, per source and
   per member override, announces `run` events (ADR-0018), whose
   attributes are `action`, `conclusion`, `workflow`, `trigger`, `branch`,
   `commit`, `number`, `attempt`, `actor` and `from`, and whose `run` link
   is the run's page.
2. **One vocabulary on every forge.** Conclusions are GitHub's words:
   GitLab's `failed`, `canceled` and `manual` read as `failure`,
   `cancelled` and `action_required`; a commit status's `error` reads as
   `failure`. GitLab's `merge_request_event` trigger reads as
   `pull_request`. One route then takes a forge's runs as another's.
3. **`runs` narrows per source.** `from` (`actions`, and on GitHub
   `checks`), `actions` (`completed` where absent), `conclusions`,
   `branches` and `workflows` as globs, and `triggers`. A single value is
   sent to the forge as a query parameter; every key applies again to
   the answer. `runs` without `run` in the signals, an empty `from` or
   `actions`, `checks` beside GitLab or Forgejo, and `conclusions` with no
   `completed` action are refused.
4. **A run's identity counts attempts where the forge does.**
   `<host>/<repository id>/run/<run id>/<attempt>`, with `check-suite` or
   `status` for checks. GitLab, Forgejo and checks name no attempt; the
   conclusion takes its place, so a run that runs again announces again
   where it ends otherwise. An unfinished run's identity ends with its
   action. The host is the repository's page's, never a status's target.
5. **Every path reads runs.** Webhooks (`workflow_run`, `check_suite`,
   `status`; GitLab's Pipeline Hook; Forgejo's `workflow_run_*` and
   `action_run_*`) announce at once; a GitHub Actions run started by one
   of them announces through `announce --from github`; a poll reads each
   listing under its own validators, kept in the cursor by signal or, for
   checks, by listing. An owner's events and feed name no runs, so an
   owner source announcing runs polls every member each round, and its
   GraphQL batch reads runs over REST beside it. A GitLab member hook
   herald registers subscribes to pipelines where the member takes runs.
6. **A route matches by attributes.** `match.attributes` takes an event
   whose every named attribute holds one of its values, as text ignoring
   case (any value where none is given). `match.not`, written as a match
   is, leaves out what it takes; it holds no `not` of its own and must
   narrow by something. An attribute no kind the route takes carries is
   refused.
7. **A route's kinds must be announced.** A route whose `kinds` names a
   kind none of its sources or apps announces is refused at load.

## Consequences

- A build log, nightly failures and red pull requests reach chat from
  `herald.toml`, with no workflow building events by hand.
- Polling checks reads one commit's suites and statuses; doctor says so
  where no webhook is named. Polling an owner's runs costs a conditional
  request per member each round.
- `match.attributes` serves every kind, so the alert, pull-request and
  issue kinds route the same way.
- A config whose route names a kind its sources never announce stops
  loading, where before the route took nothing.
