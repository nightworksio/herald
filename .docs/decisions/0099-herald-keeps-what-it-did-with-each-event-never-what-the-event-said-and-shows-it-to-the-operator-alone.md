# ADR-0099: herald keeps what it did with each event, never what the event said, and shows it to the operator alone

**Status:** Accepted
**Date:** 2026-11-27

## Context

What a run did with each route (sent, already announced, held, gathered,
stopped and why) is told in its output and log and kept nowhere, so an
operator cannot see days later why a post did not appear. Peer requests
are already kept per peer and UTC day, at most 1000 a day with the rest
counted, for `keep_audit` (90 days, at most 365), shown by `herald peer
log` (ADR-0054 decision 5). herald's commands withhold private
repositories' rows unless their output is said private (ADR-0069).

## Decision

1. **An activity record, on by default.** A new `Activity` store port in
   every store that keeps records keeps one record per UTC day: each entry
   holds the time, the run (`announce`, `poll`, `round`, `server`), the
   source, the event's identity, kind, project and visibility, and per
   route its id, its outcome (sent, already, held or gathered with when,
   quiet with until, stopped with the reason, skipped with why, refused
   with the rule that refused it, unrouted) and the steps landed. Chat
   presses (ADR-0098) and refused inbound requests (an unverified
   signature, a replay) are entries too.
2. **Never what the event said.** No title, body, private repository's
   URL, token or request body: the entry is a closed list of fields, and a
   test pins its keys.
3. **Bounds.** At most `activity.most` (2000 where absent) entries a day,
   the rest counted; `keep_activity` "14d" where absent, at most 365 days;
   `activity = false` turns it off. The markers store keeps none and says
   so.
4. **`herald status`** reads it: `--since`, `--route`, `--source`,
   `--outcome`, `--format text|json`, `--private` (private rows withheld
   and counted without it, ADR-0069); its exit code is 1 where a route
   stopped in the window. `--summary` writes the public rows alone to
   `$GITHUB_STEP_SUMMARY`, private ones counted.
5. **A status page on the server, off until `[status] serve = true`.**
   `GET /herald/status` (HTML) and `/herald/status.json` (versioned),
   rendered by a sans-IO `herald-pages` crate: server-rendered, no script,
   `Content-Security-Policy: default-src 'none'; style-src 'self';
   img-src 'self'; frame-ancestors 'none'`, herald's own stylesheet, no
   third-party font, image, script or analytics, `Referrer-Policy:
   no-referrer`, `Cache-Control: no-store`.
6. **Who may read it.** `[status] access`:
   - `"link"`, the default: `herald status --link` prints a link signed
     under a key of its own, for the status page alone, expiring in 15
     minutes;
   - `"proxy"`: an authenticating proxy's identity header, taken only from
     the proxy's address the config names;
   - `"sign-in"`: the forge's OAuth sign-in, showing rows only of
     repositories where the viewer holds `admin`.
   A JSON reader uses a token hashed in the config as an app token is
   (ADR-0015), scoped to `status:read`.
7. **This extends** ADR-0013 (refused inbound requests are recorded,
   without their bodies) and ADR-0054 decision 5's audit model.

## Alternatives

| Option | Why not |
|--------|---------|
| **Off unless turned on** | The record is for the day a post went missing, which nobody plans for. |
| **Logs only, as JSON** | An operator would need a log store of their own to look back. |
| **Keep titles and bodies** | The record would hold what private repositories said. |
| **The proxy or sign-in as default** | Each needs more set up; a signed link needs nothing. |

## Consequences

- "Why did this not post?" is answered by `herald status` for 14 days.
- A store that keeps records holds one day record more per day.

## Related

- [ADR-0013](0013-a-webhook-delivery-is-admitted-once-released-when-it-fails-and-kept-for-a-week.md): admitted deliveries; extended here
- [ADR-0015](0015-an-app-is-a-source-routes-name-announcing-with-hashed-tokens-scoped-to-its-projects.md): hashed tokens
- [ADR-0054](0054-a-peer-pushes-into-a-herald-source-it-is-granted.md): the audit model
- [ADR-0069](0069-herald-s-commands-withhold-private-repositories-rows-unless-their-output-is-said-private.md): private rows
- [ADR-0098](0098-a-chat-user-acts-on-the-forge-only-as-the-person-the-operator-names-signed-in-at-the-forge-through-an-offer-bound-to-its-post.md): presses recorded
