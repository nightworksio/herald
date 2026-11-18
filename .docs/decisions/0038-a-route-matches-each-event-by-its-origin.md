# ADR-0038: A route matches each event by its origin

**Status:** Accepted; its `match` extended by attributes and `not` by [ADR-0050](0050-a-forge-source-announces-its-ci-runs-and-a-route-matches-by-attributes.md)
**Date:** 2026-11-16

## Context

A route took the events of the sources it named, narrowed by kind,
pre-release and version. An owner source takes many repositories
(ADR-0029), and an operator who wanted "plugins to #plugins" had to split
the owner into several sources, or write a route per repository. A
template could not say which repository an event came from, and an app's
token named each project one by one (ADR-0015).

## Decision

1. **Every forge and registry event carries an origin.** `event.origin`
   holds `forge` (the source's kind), `host`, `owner`, `repository` (the
   full path or the package), `id`, `topics`, `properties`, `visibility`,
   and `source`, which herald sets to the source or app the event came
   through. A member's origin is its owner's listing; a single
   repository's is the repository answer its poll already reads, or its
   webhook's payload. A registry's owner is the package's scope, vendor or
   namespace, with no topics or properties, and public. A feed's events
   carry none.
2. **An app may name an origin.** It is checked as a forge's: a kind's name,
   a host (a domain or `localhost`, with a port), paths of names, an id,
   and 512 bytes a field at most. An app reaches only its own routes, so its
   origin only narrows where its events go.
3. **The bound is the stricter visibility.** An event reaches only the
   `private` routes where `[doctor] private_to_public = "refuse"` and its
   own visibility (a package's, ADR-0035) or else its origin's is not
   public. A single repository's events are bounded too, and a repository
   whose visibility the forge does not name counts as private.
4. **A route's `match` narrows its sources.** `sources` stays required.
   `match` takes `projects`, `repositories` and `owners` as globs, `forges`
   and `hosts` as names, `topics` (any of) and `properties` (every one, with
   one of its values). An event with no origin passes only a match that
   names projects alone.
5. **A member override names routes.** `routes` adds routes for the
   members a glob names, whatever sources those routes name; `routes_only =
   true` makes them the only ones. Each key is settled by the narrowest
   glob, as `project` is (ADR-0030). A named route still narrows by its
   kinds, pre-releases, versions and match. An override naming an id no
   route has, or a public route where its owner takes private repositories,
   does not load.
6. **An app's projects are globs.** A glob scopes what the app may post and
   claims nothing: no member is withheld for announcing as a project it
   matches, and it has no actor of its own. A project named without a
   wildcard is the app's own, as before.
7. **A per-project actor follows the match.** A route announcing each
   project as an actor of its own (ADR-0036) serves the projects its match
   may take, the members of owners its sources name or whose overrides name
   it, and its apps' own projects.
8. **A template reading the origin loads only where every event has one.**
   Each template is dry-rendered against samples from a sample origin, and
   against samples without one where the route takes an app or a feed.

## Consequences

- One route takes "plugins" across forges and registries, and a member can
  be sent somewhere else without a source of its own.
- A private single-repository source to a public place now announces
  nothing there under the default guard, where it announced before.
- `herald-event.schema.json` gains `origin`; `herald.schema.json` gains a
  route's `match` and an override's `routes` and `routes_only`.
