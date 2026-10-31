# ADR-0005: An announcement is sent once, finished if interrupted, and kept in any of nine stores

**Status:** Accepted; decision 7 superseded in part by [ADR-0011](0011-a-marker-counts-only-on-herald-s-own-post-and-only-herald-can-compute-it.md)
**Date:** 2026-10-31

## Context

An announcement posted twice is the failure everyone notices. Reruns are
routine: a workflow re-run after a flaky step, a webhook redelivered, a poll
that overlaps the last one, a serverless invocation retried by its platform.
Each of them must find that the announcement already happened.

An announcement half-posted is the failure nobody notices. On Discord an
announcement is a conversation: post, crosspost, open a thread, post the
overflow into it, pin. A run that dies after the post and before the thread
leaves a release with no thread, and no error anywhere.

The state that prevents both has to live somewhere, and herald needs nothing
long-lived (ADR-0002). An Actions-only adopter has no database. A Worker has
D1, a Lambda has DynamoDB, a container has SQLite. An adopter with no store at
all still has the destination, and the destination holds the posts.

## Decision

1. **Every announcement has a deterministic key.** The key is the SHA-256 of
   the route's `id` and the event's identity (ADR-0003), written in hex. A
   route's `id` is required and stable, so renaming a route's other settings
   changes no key.

2. **An announcement is a list of steps, and the ledger records each one that
   lands.** The plan for a route's destination lists the steps in order, for
   example: post the announcement, crosspost it, open its thread, post each
   overflow chunk, and pin it. For the ActivityPub actor there is also one
   delivery step per follower inbox. A landed step is recorded with what the
   destination answered: the message id, the thread id and the permalink.

3. **The plan reads the ledger.**
   - With no entry for the key, it plans every step.
   - With a partial entry, it plans the steps that have not landed, in order.
   - With a complete entry, it plans nothing.

   It is a pure step in the core, and property tests hold it (ADR-0008).

4. **A run claims a key before it sends, and records each step before the
   next.**
   - **The claim.** The claim is a compare-and-set that writes a lease, whose
     expiry is read from `Clock`. A concurrent run sees the lease, sends
     nothing for that key, and says so.
   - **Each step.** Each landed step is recorded with a compare-and-set
     before the next step is sent.
   - **A run that stops part-way.** It exits non-zero. It names what landed
     and what remains, and the next run sends what remains.
   - **An expired lease with an unrecorded step.** The step may have landed
     before its run died. Before resending it, herald asks the destination
     where it can: a destination marker (decision 7), a Matrix transaction id,
     a Mastodon idempotency key, or a Discord `nonce` with `enforce_nonce`.

5. **The `Store` port holds four things, with a transactional
   compare-and-set** (ADR-0001):
   - the **ledger**: entries by key, with their steps and leases;
   - **cursors**: what each polled source has seen (ADR-0003);
   - **schedules**: housekeeping that falls due, such as a thread to archive
     after seven days;
   - **subscriptions**: Slack and Matrix subscribers, and ActivityPub
     followers. Discord's subscriptions are its roles, which Discord keeps.

   Every record carries a schema version, and a store reads the versions
   before its own.

6. **Nine stores, each where it can run.**

   | Store | Kept in | Usable from | Compare-and-set by |
   |-------|---------|-------------|--------------------|
   | `sqlite` | a file, on a volume or a persistent disk | the container, and the CLI on a machine that keeps its disk | a transaction over `Sql` |
   | `postgres` | a Postgres database | the CLI, the Lambda and the container | a transaction with row versions over `Sql` |
   | `d1` | Cloudflare D1 | the Worker, through its binding | conditional statements in one batch over `Sql` |
   | `dynamodb` | an AWS DynamoDB table | the Lambda and the CLI | conditional writes, over `Http` with SigV4 |
   | `deno-kv` | Deno KV | the Deno driver | an atomic check of versionstamps over `Kv` |
   | `markers` | the destination's own posts | every layer | the destination's own idempotency |
   | `git-branch` | one file per record on a `herald-state` branch | Actions, and the CLI anywhere with a token that may push | a ref update that is refused unless it fast-forwards |
   | `actions-cache` | generations of the Actions cache | Actions | creating the next generation, which the cache service refuses if it exists |
   | `repo-variables` | repository variables | Actions, with a token that may write variables | a lock variable, whose creation the API refuses if it exists |

   SQLite, Postgres and D1 stores are written over the `Sql` port, Deno KV over
   `Kv`, and the rest over `Http`. The driver supplies the connection
   (ADR-0001).

7. **The `markers` store reads the destination's own posts.**
   - **Where the key goes.** Each announcement carries its key in the most
     invisible place the platform keeps (ADR-0004).
   - **The ledger.** The store finds the key among the destination's recent
     posts. It reads which steps landed from the post itself: whether it was
     crossposted, whether it has a thread, and whether it is pinned. A rerun
     is therefore safe with no store but the destination.
   - **What it cannot hold.** It holds no cursors and no herald-kept
     subscriptions, because a destination has nowhere to keep them.
   - **Schedules.** It derives schedules from the threads the destination
     lists.
   - **What doctor names.** A config that polls or keeps subscribers on
     `markers` alone is named by doctor (ADR-0002). So is a destination that
     cannot read back its posts, such as a webhook.

8. **One conformance suite holds every store.**
   - **Where it lives.** The suite is in `herald-testing` and is parameterised
     by the capabilities a store declares: ledger, cursors, schedules,
     subscriptions, and compare-and-set under concurrency.
   - **What passing means.** Every store passes every case for every
     capability it declares, and the fake store passes all of them.
   - **Where it runs.** In CI the suite runs against each store's real
     backend or the platform's local emulator: SQLite in process, Postgres in
     a service container, DynamoDB Local, D1 and Deno KV in their local
     runtimes. The GitHub-backed stores run against a fake of the API that
     holds the API's refusals, and live in herald's own workflows
     (ADR-0010).

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **A random id per announcement** | A rerun cannot recompute it, so the ledger could never say "already sent". |
| **Recording an announcement as one unit** | A run that dies between the post and the thread would either repeat the post or leave the thread missing. Recording each step lets the next run finish exactly what is left. |
| **No claim, only the ledger** | Two overlapping runs would both read "not sent" and both post. The claim makes one of them wait. |
| **One store for every layer** | No single store exists on a runner with no service, in a Worker and in a container. Nine stores over one port and one suite let each layer use what it has. |
| **Workflow `concurrency` instead of compare-and-set for the GitHub-backed stores** | It serialises runs of one workflow and not runs of two, and herald may be called from several. Each GitHub-backed store uses the one atomic operation its API offers. |
| **Markers as the only never-twice mechanism** | A webhook cannot read back its posts, a poll needs a cursor, and subscriptions need somewhere to live. Markers are the store of last resort, and a check before resending. |

## Consequences

**Reruns are free.** A redelivered webhook, a re-run workflow and an
overlapping poll all find the key and plan nothing.

**Interruptions heal.** The run after a failure finishes the thread, the
chunks or the deliveries, and sends nothing twice.

**A store is chosen by where herald runs.** The conformance suite makes the
choice one of capability, not of correctness.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): the `Store`, `Sql`, `Kv` and `Markers` ports
- [ADR-0003](0003-every-source-becomes-one-neutral-event.md): identities and cursors
- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): steps, markers and each platform's idempotency
- [ADR-0008](0008-every-line-is-covered-and-no-mutant-survives.md): property tests for the plan, and the conformance suite in CI
- [ADR-0011](0011-a-marker-counts-only-on-herald-s-own-post-and-only-herald-can-compute-it.md): a marker counts only on herald's own post, and only herald can compute it
