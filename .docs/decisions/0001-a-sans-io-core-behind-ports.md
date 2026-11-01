# ADR-0001: A sans-IO core behind ports, in crates that each name only those before them

**Status:** Accepted
**Date:** 2026-10-31

## Context

herald announces what happened (a release, a new package version, a feed
entry, or anything an app says) to the places people gather: Discord, Matrix,
Slack and the Fediverse. Each announcement is formatted for its destination,
threaded where the destination can thread, and never sent twice. Easy to adopt
is the core requirement: one `herald.toml`, and the same config runs as a
GitHub Action, a serverless endpoint or a container (ADR-0002).

That puts the same decisions on very different machines. A GitHub runner has a
filesystem, a shell and an hour. A Cloudflare Worker has no filesystem and no
threads, and the only I/O it has is `fetch`. A Lambda has a filesystem but
nothing that outlives the invocation. A container has everything. If routing,
rendering, chunking and the never-twice plan were written against any one of
them, every other runtime would get a port of that code, and the ports would
drift.

The approved shape is a sans-IO core with ports around it. The core decides,
and is handed everything it reads. Ports describe the outside world, and only
drivers touch it. This ADR fixes where the lines run.

## Decision

1. **The workspace is layered, and a crate names only itself and the layers
   before it.** In order:

   | Layer | Crates | Holds |
   |-------|--------|-------|
   | core | `herald-core` | The domain, with no I/O |
   | ports | `herald-ports` | The traits through which everything outside is reached |
   | testing | `herald-testing` | A fake of every port, the `Http` that replays recorded fixtures, and the shared conformance suites. A dev-dependency only |
   | kits | `herald-records`, `herald-sql` | What several stores share: `herald-records` is the `Store` kept one record per path over any backend that replaces a record only as it was read, which the git-branch, Actions-cache and repository-variables stores are; `herald-sql` is the schema and statements the SQLite, Postgres and D1 stores run over `Sql` |
   | sources | `herald-sources-forge`, `herald-sources-registry`, `herald-sources-feed`, `herald-sources-app` | One crate per source family (ADR-0003) |
   | destinations | `herald-dest-discord`, `herald-dest-matrix`, `herald-dest-slack`, `herald-dest-fediverse` | One crate per destination (ADR-0004) |
   | stores | `herald-store-sqlite`, `herald-store-postgres`, `herald-store-d1`, `herald-store-dynamodb`, `herald-store-deno-kv`, `herald-store-markers`, `herald-store-git-branch`, `herald-store-actions-cache`, `herald-store-repo-variables` | One crate per store (ADR-0005) |
   | config | `herald-config` | `herald.toml` read into typed config, the JSON Schema, and templates compiled at load (ADR-0006) |
   | flows | `herald-flows` | The commands as flows over the ports: announce, poll, housekeep, interact, doctor and preview |
   | native | `herald-native` | The ports implemented with Tokio, `reqwest` with rustls, the filesystem and the environment, shared by the native drivers |
   | drivers | `herald` (the CLI), `herald-worker`, `herald-lambda`, `herald-vercel`, `herald-netlify`, `herald-deno`, `herald-server` | The composition roots |

   The native and driver layers are the only crates with an async runtime and
   real I/O, and the rules below call them the I/O layers.

   Within the sources, destinations, stores and drivers layers, a crate names
   no sibling. A source knows nothing of Discord, Discord knows nothing of
   SQLite, and the Worker knows nothing of the Lambda. What they share lives
   in an earlier layer: what several stores share is a kit, and each store
   keeps its own crate and passes the conformance suite on its own.

2. **`herald-core` decides, and does nothing else.** It holds these values:
   - `Event`: source-neutral, with a kind, project, version, title, body,
     links, a pre-release flag and when it occurred;
   - `Route`: an event filter leading to a destination, a template and options;
   - `Announcement`: an event rendered for one route;
   - `Profile`: a destination's limits and features, as data;
   - `Ledger` entry, `Cursor` and `Schedule`.

   It also holds the steps that turn one value into the next:
   - *normalise*: a source's parsed payload becomes an `Event`;
   - *route*: an event becomes the announcements its routes ask for;
   - *render*: minijinja, per destination dialect (ADR-0006);
   - *chunk*: what exceeds a destination's limits is split where a reader
     would split it;
   - *plan*: what to send, given the ledger (ADR-0005);
   - *housekeep*: the schedules that are due become actions.

   It compiles to `wasm32-unknown-unknown` and to native, and CI builds both.
   It has no async runtime, no clock and no randomness. It does not read a
   file, the environment or the network. The time, the ledger, the config and
   the payloads are handed in as values, and everything it produces is a
   value.

3. **The ports, and what each one answers.** The methods are `async` but name
   no runtime, so the same trait is served by Tokio on a runner and by the
   JavaScript event loop in a Worker.

   | Port | Answers |
   |------|---------|
   | `Http` | One request, one response. The request is a value, and so is the response. Retries, timeouts and the outbound host allow-list belong to the implementation (ADR-0007) |
   | `Store` | The ledger, cursors, schedules and subscriptions, with a transactional compare-and-set (ADR-0005) |
   | `Clock` | The current instant |
   | `Secrets` | A secret's value, by the name the config gives it (ADR-0007) |
   | `Signer`, `Verifier` | HMAC-SHA256, Ed25519 and, for ActivityPub's HTTP signatures, RSA-SHA256. The signer signs with a key it holds, and the verifier checks a signature against a key or secret it is given |
   | `Sql` | Statements with typed parameters, run in one transaction, answered by rows. SQLite, Postgres and D1 stores are written over it |
   | `Kv` | Versioned reads and an atomic check-and-set over keys. Deno KV is written over it |
   | `Markers` | The never-twice keys a destination's recent posts carry. Each destination crate that can read its own posts implements it (ADR-0005) |

   `Sql`, `Kv` and `Markers` exist so that no store crate needs a client
   library of its own. The driver connects to SQLite, Postgres, D1 or Deno KV
   with the platform's own client, and hands the store a port.

4. **Each destination's limits and features are data.** A destination crate
   offers a `Profile`: its text and title lengths, embed limits, whether it
   threads, crossposts, edits, pins and reads back its own posts, how it
   mentions people, and which dialect it renders. The core plans against the
   `Profile` and never names a platform.

5. **Sources are pure parsers.** A source crate parses a forge payload, a
   registry response, a feed or an app's event into `Event`. Where it must ask
   the outside world, it returns the description of the request to make, and
   parses the response it is handed back. It does not call `Http` itself.

6. **Destinations reach their platform through `Http`, and nothing else.** A
   destination crate turns a planned announcement into requests, sends them
   through `Http`, and reads the responses into ledger facts: the message id,
   the thread id and the permalink.

7. **The flows are written once.** `herald-flows` composes the steps and the
   ports into the commands every driver offers: *announce*, *poll*,
   *housekeep*, *interact*, *doctor* and *preview*. Each driver constructs the
   ports for its platform and hands them to the same flow. The CLI on a
   runner and the Worker on Cloudflare run the same announce flow.

8. **Drivers are the composition roots.** Each is the only place its ports are
   constructed. The CLI, the Lambda and the server construct theirs from
   `herald-native`:
   - `herald` is the CLI;
   - `herald-worker` is built on `workers-rs`, with `fetch`, D1 and the
     Worker's secrets;
   - `herald-lambda` is the AWS Lambda entry point;
   - `herald-vercel`, `herald-netlify` and `herald-deno` are thin entry
     points around the wasm build, each with the platform's `fetch` and
     secrets;
   - `herald-server` is the container's server, on axum.

9. **What holds the boundary.** These rules are enforced, and each names its
   enforcer:
   - **The layer order, and no sibling dependencies.** An arch test reads
     `cargo metadata` and refuses any edge between workspace crates that the
     table in decision 1 does not allow.
   - **No runtime or I/O crate outside the I/O layers.** The same arch test
     holds an allow-list of outside crates per layer. Tokio, `reqwest`,
     `rusqlite`, `tokio-postgres`, `axum`, `workers-rs` and `lambda_runtime`
     are allowed only in the I/O layers.
   - **No ambient I/O outside the I/O layers.** Clippy's `disallowed-methods`
     and `disallowed-types`, configured for every crate before them, refuse
     the filesystem, the network, processes, the environment, the system clock,
     sleeping and randomness.
   - **The core is portable.** CI builds `herald-core` for
     `wasm32-unknown-unknown`.
   - **Every port has a fake.** `herald-testing` has a hand-written fake of
     each port. Every implementation of a port passes the same suite that its
     fake passes.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **One binary with the I/O inline** | The fastest to write first, and the one that fits only one runtime. A Worker cannot run Tokio, open a file or spawn a thread, so the Worker would have needed a second implementation of every decision. |
| **An async core** | It makes every core function generic over a runtime or tied to one. It drags futures into code that only computes. The core has nothing to wait for once it is handed its inputs, so it waits for nothing. |
| **Destinations as sans-IO request builders, like sources** | A destination's work is a conversation: post, read the id, crosspost that id, open a thread on it, post the rest into the thread. Each request depends on the last answer. Written as builders, the conversation would move into every driver. Through `Http` it is written once and still runs everywhere. |
| **Store crates with their own database clients** | `rusqlite` and `tokio-postgres` cannot run in a Worker, and D1 and Deno KV exist only inside their platforms. The narrow `Sql` and `Kv` ports keep each store's logic in one place, testable over a fake, and let the driver supply the connection. |
| **One crate for all destinations, behind features** | Features are additive, so the arch test could not prove that Discord's code never names Slack's. Separate crates make that boundary a compile error. |
| **cargo-deny's `bans` or a third-party layering tool for the rules** | Neither expresses "this crate may name only those layers". The arch test is a plain test over `cargo metadata`, holding the same table this ADR does. |

## Consequences

**The decisions are testable without a network.** Routing, rendering,
chunking and planning run over values in microseconds. Every destination is
proved against recorded fixtures, and every store against the shared
conformance suite (ADR-0008).

**A new runtime is a new driver and nothing else.** Nothing before the
I/O layers names Tokio, the filesystem or a platform, and the arch test fails
the day something does.

**A new source, destination or store is one new crate.** It names the core,
the ports and nothing beside it.

**The public API is the core's values, the ports, and the flows' entry
points.** From 0.1.0, changing any of them follows semantic versioning
(ADR-0009).

## Related

- [ADR-0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md): the layers the drivers serve
- [ADR-0003](0003-every-source-becomes-one-neutral-event.md): the sources and the `Event` they produce
- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): the destinations and their profiles
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the `Store` port and the plan
- [ADR-0008](0008-every-line-is-covered-and-no-mutant-survives.md): the toolchain that enforces these rules
