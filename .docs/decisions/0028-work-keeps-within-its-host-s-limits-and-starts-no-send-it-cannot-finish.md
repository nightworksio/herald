# ADR-0028: Work keeps within its host's limits and starts no send it cannot finish

**Status:** Accepted
**Date:** 2026-11-14

## Context

herald answers a request first and does the work it leaves after the answer
(ADR-0014). Some hosts bound that work. Cloudflare Workers' `waitUntil` lets
it run at most 30 seconds after the answer
([limits](https://developers.cloudflare.com/workers/platform/limits/)), and
Fastly Compute lets a request make at most 32 backend requests, 10 on a
trial
([resource limits](https://docs.fastly.com/products/compute-resource-limits)).
A send the platform cuts off is left attempted in the ledger, so the next run
must read the destination back to learn whether it landed, and where the
destination cannot be read back the step is not sent again (ADR-0005,
ADR-0022).

## Decision

1. **`[server.work]` holds four settings.** `after` bounds how long the work
   a request leaves runs after its answer, and `requests` how many outbound
   requests the request and its work make, counted from when herald began
   answering it. `spare`, "10s" where absent, and `spare_requests`, 4 where
   absent, are how much of each must be left for a send to start.
2. **`after` and `requests` default to the platform's own limit**: 30
   seconds on Cloudflare Workers, 32 requests on Fastly Compute, and
   unbounded on every other host. The limits are described with each host
   (ADR-0024).
3. **Every outbound request herald makes is counted**: each route's,
   source's and store's connection and the public fetches share one count.
4. **A route checks the budget before each step, and before each batch of
   ActivityPub deliveries**, and a poll checks it before each source. Where
   the step could pass a limit, the route stops with `Limit`, naming what
   was left, before the step is marked attempted. The next run sends the
   remaining steps without reading the destination back.
5. **A config whose spare leaves no room is refused**: by the loader where
   both values are given, and when herald is composed where the limit is the
   host's.

```toml
[server.work]
after = "30s"
spare = "10s"
requests = 32
spare_requests = 4
```

## Consequences

**An announcement cut short by a host's limit is handed over cleanly**: what
was sent is in the ledger, and nothing is left half sent.

**A host's limits that change, or a trial's lower ones, are one setting
away.**

**A request near Fastly's limit stops early rather than late**: the count
includes every request herald made since it began answering, among them the
store's.

## Related

- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the ledger the next run finishes from
- [ADR-0014](0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md): answering first and working after, extended here
- [ADR-0024](0024-a-host-is-described-once-and-every-driver-composes-through-one-composition.md): where each host is described
