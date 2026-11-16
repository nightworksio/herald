# ADR-0036: A route announces each project as an actor of its own

**Status:** Accepted
**Date:** 2026-11-16

## Context

An `activitypub` route announced as one actor, named by the route
(ADR-0004). An owner source takes many repositories (ADR-0029), and a
follower who wants one of them had to follow every one, or the operator
had to write a route per repository. WebFinger and actor documents are
asked for before any event, so herald must know each actor it serves in
advance.

## Decision

1. **`actor` is a name or a template.** A route's `actor` written with
   `{{`, `{%` or `{#` is a template that names each project's actor. It
   reads `event.project` alone, the one field known of a project before
   its events, and the config does not load where it reads anything else
   or does not compile. One actor for the route stays the default.
2. **The actors are the projects the route can announce.** A route naming
   each project's actor serves an actor for each project its sources name,
   each project of the apps announcing to it, and each member its owner
   sources keep. A member not everyone may see has an actor only where it
   may reach the route (ADR-0029).
3. **A name breaking the username rule skips the route.** A project whose
   rendered name is not an actor's name (at most 30 lowercase letters,
   digits, `_` or a `-` within) is not announced on the route, and the run
   says so. doctor names each such project of a source or app, and online
   each such member, with the rule.
4. **A member that leaves keeps its actor while it has followers.** Its
   actor keeps the followers it has, takes no new follows (each is
   answered with a `Reject`, and the operator cannot approve one waiting),
   and announces nothing. Once its last follower leaves, or its owner's
   `keep_members` passes, the actor is gone: herald answers 410 for it,
   as it does for a name followers still follow that no project names.
5. **Each event announces as its project's actor.** Its Note, outbox and
   deliveries are that actor's, delivered to that actor's followers alone.
6. **A name is one actor's.** Actors on one domain share one namespace. A
   route's own actor keeps its name: a project naming it is not announced
   on its route. A name that two sources, members or apps name, or that
   routes' own actors on two destinations of one domain hold, is served
   as none of theirs (404, naming them), and no project naming it is
   announced on its route. Each announcement checks this against the
   members the store keeps, and stops for the next run where they cannot
   be read. doctor names each such name with the routes and projects
   holding it, and online each one a member holds.

## Consequences

- One route serves one actor per repository of an owner, each followed on
  its own.
- Serving such an actor, and announcing as one, reads the members of the
  owners of every such route on its domain from the store.
- A repository whose name renders to another's actor cannot take that
  actor's followers or post as it.
- Route `match` filters and member overrides that name routes narrow which
  members reach a route; once they exist, they narrow its actors too.
