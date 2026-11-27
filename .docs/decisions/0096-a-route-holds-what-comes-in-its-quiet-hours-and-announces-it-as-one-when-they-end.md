# ADR-0096: A route holds what comes in its quiet hours and announces it as one when they end

**Status:** Accepted
**Date:** 2026-11-27

## Context

A route gathers events that share a key for a window and announces them
as one (ADR-0045), in a held record of the `Waiting` port that a round
announces once it is due. Reports have their own `at`, `days` and `zone`,
read by `report::zone` from the bundled time zone database. Nothing keeps
a chat quiet at night or at weekends. Slack can schedule a message itself
with `chat.scheduleMessage`, up to 120 days ahead and at most 30 into one
channel within five minutes
(<https://docs.slack.dev/reference/methods/chat.scheduleMessage>).

## Decision

1. **`quiet` on a destination, for every route to it, or on a route,
   which wins:**

   ```toml
   [destinations.team-slack.quiet]
   between = ["19:00", "08:00"]
   days = ["mon", "tue", "wed", "thu", "fri"]
   weekends = true
   zone = "Europe/Amsterdam"
   deliver = "one"
   urgent = ["alert"]
   most = 50
   edits = "send"
   ```

   `between` may cross midnight; `days` are the days a window starts on,
   every day where absent; `weekends` also keeps Saturday and Sunday quiet
   (false where absent); `zone` is `"UTC"` where absent. Several windows
   are a list of such tables. A route says `quiet = false` to ignore its
   destination's.
2. **What comes in a quiet window is held.** After routing and every
   privacy check, a route inside its window holds the event in a new
   `Hold::Quiet { route, window_start }` record due at the window's end,
   computed as a report's start is, so a skipped or repeated wall-clock
   time is met once. The outcome is told as "held for quiet hours until
   08:00 Europe/Amsterdam". The first round at or after the end announces
   what is held: `deliver = "one"` (the default) as one group through the
   route's template, `"each"` one by one in order. Past `most` (50 where
   absent) the rest go in a second message. A group that falls due inside
   a window waits for its end.
3. **Security alerts are never held.** `urgent` is `["alert"]` where
   absent; the operator may add kinds or remove `alert`. A `quiet` on a
   route whose kinds are all urgent does not load.
4. **Edits, withdrawals, pins and housekeeping go through** where
   `edits = "send"` (the default); `"hold"` holds edits too.
5. **herald holds posts itself, on every destination.** It never hands a
   held post to a platform's scheduler: what herald holds it can still
   group, edit and withdraw, and it re-checks before it sends that the
   config still lets the event reach the route, dropping one that may no
   longer.
6. **A group may close when a burst ends.** ADR-0045's `group.until`
   gains `"calm"`, with `calm = "2m"` where absent: the group is due once
   no member came for `calm`, and never later than `window`.
7. **Where held records live.** In every store that keeps records; a
   route with quiet hours does not load on the markers store, and the
   error names the stores that keep records. (A grouped route there stops
   for the round, ADR-0045 decision 9; a quiet window is known as the
   config loads, so it is refused then.)
   Held records are sealed as every record is (ADR-0005) and dropped once
   announced.
8. **Each host releases them at its next round**: `herald server` also
   sleeps to the earliest due it holds; a serverless host at its next
   scheduled round; `wasi:http`, Spin and Fastly at the next
   `POST /herald/round/*`; Actions at the next scheduled `poll` or
   `housekeep` run, or the next event's run, since `announce` releases
   what is due for its routes first. Doctor says the latency per host,
   and a report whose `at` falls inside the quiet window of every route
   naming it.
9. **Refused at load:** `between` with equal ends; a zone the build cannot
   read (naming the `tzdb` feature); `deliver = "one"` on a route that
   announces each project as an actor (as ADR-0045 decision 8 refuses
   groups there); `quiet` on a route to a herald feed, which publishes each
   event as an envelope of its own (as decision 8 refuses groups there).

## Alternatives

| Option | Why not |
|--------|---------|
| **Nothing exempt unless named** | An alert about a leaked secret would wait for morning. |
| **Alerts and failed CI runs on the main branch never held** | A failed run waits well until the morning; the operator can add `run` to `urgent`. |
| **Slack's own scheduler on Slack bots** | A post handed to Slack leaves herald's control: it could no longer be grouped, edited, withdrawn or re-checked, and webhooks and other chats would behave differently. |

## Consequences

- A team's chat is quiet in its own hours, and what came in them is one
  message when they end.
- Quiet hours need a store that keeps records, and on Actions a scheduled
  run to be on time.

## Related

- [ADR-0045](0045-a-route-gathers-the-events-that-share-a-key-into-one-announcement.md): groups; extended here
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): stores and sealing
- [ADR-0051](0051-a-forge-source-announces-its-security-alerts-to-private-places.md): security alerts
- [ADR-0055](0055-a-report-announces-a-view-of-open-pull-requests-once-a-window.md): reports' schedules
