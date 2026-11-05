# ADR-0015: An app is a source routes name, announcing with hashed tokens scoped to its projects

**Status:** Accepted
**Date:** 2026-11-05

## Context

ADR-0003 lets any app announce an event of its own, written as herald's
event JSON, through `herald announce --event` and the HTTP API. ADR-0007
gives each app a token, kept by its SHA-256 under `[apps.<name>]` with the
projects and routes it may announce to, and refuses an event outside that
scope before it is read. Neither says how an app's event reaches a route,
how a token is rotated, where the HTTP API answers, or what a repeated
event is answered.

A route takes events from the sources it names (ADR-0006). An app is not a
source of `[sources]`: nothing is polled or hooked for it.

## Decision

1. **An app's event is herald's event JSON.**
   - `herald-event.schema.json`, generated from the same type the app
     source reads, describes it: `id`, `project`, `title`, and optionally
     `version`, `body`, `links`, `prerelease` and `occurred`. A field it
     does not name is refused.
   - The event is a `message`. Its identity is the app's name and the `id`
     it gives (`app/<app>/<id>`), so an app that sends one event twice is
     announced once (ADR-0005). Where it names no time, it happened when
     herald read it.
2. **An app is a source routes name.**
   - A route's `sources` may name an app as it names a source, and the two
     share one namespace: an app named like a source is refused.
   - `[apps.<name>] routes` narrows the routes an app announces to, each
     one naming the app. Where it names none, the app announces to every
     route that does.
   - `projects` names the projects whose events the app may announce.
3. **An app holds tokens, kept by their hashes.**
   - `herald token new --app <name>` prints a new 256-bit random token once,
     with the line herald.toml keeps: `tokens = ["sha256:<hex>"]`.
   - `tokens` is a list, so a token is rotated by adding the new one before
     removing the old. Every presented token's hash is compared with every
     hash every app keeps, each in constant time.
   - An app without tokens is announced from the CLI alone.
4. **The HTTP API answers at `/api/projects/<project>/events`.**
   - `POST` with `Authorization: Bearer <token>` is checked against the
     token and its projects before its event is read, and answered 401 or
     403 otherwise. The event must name the path's project.
   - The event is admitted once by its identity under the `app` scheme
     (ADR-0013). A first delivery is answered 202 and announced after the
     reply (ADR-0014); one admitted already is answered 200; one whose
     announcement stopped is released, so its next delivery is admitted.
   - `GET` of `/api/projects/<project>/events/<id>`, with the same token
     and scope, answers where the event's announcement stands on each route.
5. **The CLI announces from a file or standard input.** `herald announce
   --event <file|-> --app <name>` reads an event, holds it to the app's
   projects and announces it to the app's routes, as the HTTP API does.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **Routes listed only in `[apps]`** | A route's `sources` would no longer say every event it takes, and an app could reach a route that names none of its own. |
| **One token per app** | Rotating it means a window where the app is refused, or a config change and a restart at the same moment. |
| **The project read from the event's body** | ADR-0007 refuses an out-of-scope event before it is read; with the project in the path, the scope is checked first. |
| **Answering once the announcement is done** | A slow destination would hold the app's request for as long as any route takes (ADR-0014). |

## Consequences

**Every route naming an app takes its messages**, filtered by kind and
prerelease as any source's events are. An app's events carry no version a
route's `versions` can match, as feed entries do not.

**The store keeps an app's deliveries under `app`** for a week (ADR-0013),
beside the forges' and ActivityPub's.

## Related

- [ADR-0003](0003-every-source-becomes-one-neutral-event.md): the app source and the event JSON
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): one announcement per identity and route
- [ADR-0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md): routes, sources and the published schemas
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): app tokens and their scope
- [ADR-0013](0013-a-webhook-delivery-is-admitted-once-released-when-it-fails-and-kept-for-a-week.md): admitting and releasing deliveries
- [ADR-0014](0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md): answering first and working after
