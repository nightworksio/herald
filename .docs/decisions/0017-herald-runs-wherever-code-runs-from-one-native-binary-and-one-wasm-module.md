# ADR-0017: herald runs wherever code runs, from one native binary and one wasm module

**Status:** Accepted; `Hosting` extended to every host, its stores derived from what it offers, by [ADR-0024](0024-a-host-is-described-once-and-every-driver-composes-through-one-composition.md); the `Limits::asked` it names is `Limits::timers`
**Date:** 2026-11-06

## Context

ADR-0001 names a driver per serverless platform: `herald-worker` on
workers-rs, `herald-lambda`, and thin `herald-vercel`, `herald-netlify` and
`herald-deno` entry points around a wasm build. ADR-0005 names the stores
those platforms keep records in, and ADR-0009 the artefacts that carry them.

What sets herald apart is how many ways it runs (ADR-0002). Five platforms
leave out hosts that run the same kinds of code: Azure Functions, any
JavaScript runtime such as Node or Bun, the WebAssembly component hosts of
`wasi:http`, and Fastly Compute. A driver per platform would also write the
same composition, of config, stores, ports and serving, once each.

A serverless request is answered before its work is done (ADR-0014), and
each platform finishes that work differently: a Worker, Vercel and Netlify
let a request's work outlive its answer, a Lambda freezes once it answers,
and Azure's timers call the function to poll.

## Decision

1. **One native binary runs every native host.** The CLI is the driver for
   GitHub Actions and any CI, for `herald server` on a host or in the
   image, for Azure Functions, and for AWS Lambda:
   - **Azure Functions** runs `herald server` as its custom handler.
     `herald server` listens on `FUNCTIONS_CUSTOMHANDLER_PORT`, and Azure's
     timer functions `poll` and `housekeep` ask for a round by posting to
     `/poll` and `/housekeep`. Outside HTTP is served under the app's route
     prefix, `api` by default, so no request from outside reaches those
     paths; with an empty prefix herald runs no round there. A host's own
     timers ask for rounds through `Limits::asked`, and each kind of round
     runs one at a time.
   - **AWS Lambda** runs `herald lambda` as its custom runtime. It answers
     each request to the function's URL at once, and hands the work a
     request leaves to the function again as an event invocation. That
     invocation answers the request again, its delivery admitted again
     (`Admission::Again`), and does the work. Where the function cannot
     hand it on, it does the work before answering. A scheduled event
     polls, keeps house and forgets the keys read longest ago.

2. **One wasm module runs every JavaScript host.** `herald-wasm` is built
   with wasm-bindgen behind a host interface: `fetch`, Web Crypto,
   secrets, `Sql`, `Kv`, `waitUntil` and WebSockets. A thin JavaScript shim
   in `deploy/` adapts each host to it:
   - **Cloudflare Workers**, with D1, and a Durable Object that holds the
     Discord gateway and Slack's Socket Mode sessions and can keep the
     records in its own storage;
   - **Vercel, Netlify and Deno Deploy**, with Deno KV on Deno;
   - **Node and Bun**, with SQLite or Postgres through the host's own
     client.

   It signs and verifies with the host's Web Crypto. The native binary
   keeps ring. `herald_compose::Hosting` names each JavaScript host and the
   store kinds it keeps records in, and the shim names its host to the
   module. Where a host resolves names itself, it connects on others'
   behalf only where every address a name resolves to is public. Where it
   resolves them out of herald's sight, as `wasi:http`, Spin and Fastly do, it
   connects on others' behalf only once `HERALD_EGRESS_GUARDED` says its own
   egress refuses private addresses.

3. **`wasm32-wasip2` and Fastly Compute get a driver each.** `herald-wasi` is
   a `wasi:http` component, serving `wasmtime serve`, wasmCloud and any other
   `wasi:http` host with the stores that need only `Http`. Built with its `spin`
   feature it also keeps records in the SQLite and Postgres Spin offers, through
   `herald-spin`, which holds the generated bindings apart from the code that
   forbids `unsafe`. `herald-fastly` serves Fastly Compute on Fastly's own ABI,
   with its KV Store. Both sign with `herald-crypto`, on `ring`. Neither has a
   timer, so each runs a round when an HTTP request holding `HERALD_ROUND_SECRET`
   asks; with the secret unset no request does. Every other host answers
   that request too, at `POST /herald/round/<kind>`.
4. **Every driver shares one composition.** `herald-compose`, a layer of
   its own between flows and native, holds the `Host` trait a platform
   implements: its `Http`, secrets, clock, signer and verifier, its
   variables, and the databases and key-value stores it offers. Over a
   host it reaches the store the config names, of the kinds the driver
   serves, and `Composed` answers each request, does the work a request
   leaves, polls and keeps house. Where a platform offers no database or
   key-value store, it names `Unoffered`. The native binary reaches its
   stores through it too, and keeps the Actions cache, which only a
   workflow run reaches, to itself. It caps the requests sent to
   ActivityPub servers with `async-lock`, which runs on no runtime.

   **Sessions are a flow.** `herald_flows::Sessions` holds the gateway,
   Socket Mode and a Matrix sync over `Sockets`, `Http` and a `Timer` port,
   handing each command's work to a `Taker`: `herald server`'s queue, or
   the Durable Object. Flows may name `futures-util`, which runs on no
   runtime of its own.

5. **Five stores join ADR-0005's.**

   | Store | Where | Compare-and-set |
   |-------|-------|-----------------|
   | `d1` | Cloudflare D1, through the Worker's binding | conditional statements in one batch over `Sql` |
   | `durable-object` | the storage of herald's Durable Object | a transaction of the object's storage over `Sql` |
   | `dynamodb` | an AWS DynamoDB table, for the Lambda, the CLI and the server | conditional writes over `Http`, signed with Signature Version 4, each naming the epoch and the version it replaces |
   | `deno-kv` | Deno KV | an atomic check of versionstamps over `Kv` |
   | `fastly-kv` | a Fastly KV Store | a generation match over `Kv` |

   `Kv` deletes a key only against its versionstamp. Every DynamoDB
   credential is hidden by name, whether the config names it or leaves the
   one AWS sets.

## Options considered

| Option | Why it lost |
|--------|-------------|
| **A `herald-lambda` crate, as ADR-0001 had** | It would compose the stores, secrets and connections the CLI already composes, and ship a second native binary to sign. |
| **workers-rs for the Worker, and a wasm module for the rest** | Two Rust drivers would cover the same ground: one for the Worker's bindings, one for every other JavaScript host. |
| **Pure-Rust crypto in the wasm module** | The `rsa` crate carries the Marvin timing advisory for signing (RUSTSEC-2023-0071). Web Crypto is constant-time, audited, and in every host. |
| **Doing a Lambda's work before answering** | Discord waits three seconds for a command's answer, and GitHub ten for a webhook's. |
| **Serializing a job between Lambda invocations** | Every type a job holds would need a wire form, its secrets included. The request is already one. |

## Consequences

**herald runs in more places than any one platform list names.** One
binary and one module cover the native hosts and the JavaScript ones, and a
host that is neither runs the WASI component.

**`herald doctor --layer lambda` names what a Lambda cannot serve**: a
session, and SQLite on a file system that does not outlive the function.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): the drivers, superseded in part here
- [ADR-0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md): the layers each driver serves
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the stores, extended here
- [ADR-0009](0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md): the artefacts that carry each driver
- [ADR-0014](0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md): answering first and working after
- [ADR-0016](0016-commands-are-carried-every-way-a-destination-offers-and-answered-from-what-herald-keeps.md): the sessions the Durable Object holds
- [ADR-0024](0024-a-host-is-described-once-and-every-driver-composes-through-one-composition.md): every host described once, and every driver composed through `Composed`
