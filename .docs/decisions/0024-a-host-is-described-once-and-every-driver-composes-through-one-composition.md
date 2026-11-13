# ADR-0024: A host is described once, and every driver composes through one composition

**Status:** Accepted
**Date:** 2026-11-13

## Context

ADR-0017 has every driver share one composition, `herald-compose`, and has
`herald_compose::Hosting` name each JavaScript host and the store kinds it
keeps records in. The native hosts were described a second time in the CLI:
the CLI, `herald server`, `herald lambda` and Azure Functions each had a
store list and a doctor layer of their own. Every host's store list was
written by hand, and repeated the four stores that need only `Http`. The
deploy README repeated them a third time.

The CLI composed its runs itself, beside `Composed`. It chose the keyring,
built the ports and the keys, and served over them, and `herald server` did
the same. The keys kept in the store were forgotten by `herald server`,
`herald lambda` and the wasm hosts, but never by `herald housekeep`.

## Decision

1. **`Hosting` describes every host once.** Each of the twelve hosts that
   `herald doctor --layer` names is a `Hosting`, from `actions` to `server`.
   It gives the host's name, what herald runs as there (the native binary,
   the wasm module, the `wasi:http` component or Fastly Compute), what it
   offers a store beyond `Http`, its layer of ADR-0002, whether it serves
   herald's endpoint and holds sessions, its egress, what asks it for rounds
   and the secrets a round is asked with. Doctor's layers are read from it,
   and the CLI's `--layer` lists its names. A driver accepts only the hosts
   its own artefact runs on.
2. **A store kind says what it needs, and a host's stores follow.** Each
   `StoreKind` names the `Offer` it needs: `Http` alone, a lasting SQLite
   file, Postgres, D1, a Durable Object's storage, Deno KV, a Fastly KV
   Store, or the cache of the workflow run. The stores a host keeps records
   in are the registered kinds whose needs it offers. Only the CLI, in a
   workflow run, offers the Actions cache. A test holds the deploy README's
   table to each host's stores.
3. **Every driver runs through `Composed`.** The CLI's `announce`, `poll`,
   `housekeep` and `followers`, `doctor --post`, `herald server` and
   `herald lambda` compose herald on the native host with `Composed`, as
   the wasm module, the WASI component and Fastly Compute do. `Composed`
   reads the secrets of what the run does, and keeps its records in memory
   for a dry run. Work outside its own rounds runs through it with the
   `Through` trait, over the store, the keyring and the ports it chose.
4. **Housekeeping forgets the keys due a reread.** `housekeep` forgets
   every key read longer ago than `[server.keys] refreshed` before it does
   its chores, on every host and from the CLI. `herald server` also forgets
   them on that period of its own.

5. **A destination sets itself up.** A destination that takes commands
   gives a `Setup`, without I/O: what a `SetupTask` reads (each secret, and
   how many new tokens it hands out), the steps it takes (lines to print, or
   a request to send), and what the response to that request says. The
   `herald set` subcommands name a task and run its steps. They name no
   destination's options. The installer adds its questions and discovery to
   the same trait.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **A store list per host, tested against the kinds** | The lists would still be written by hand, and a new store kind would be added to each host that offers what it needs. |
| **A `StoreKind` enum** | ADR-0018 makes every kind data that its crate supplies through a registry; an enum would name every store in one place again. |
| **Setup written in the CLI, per destination** | The CLI would read each destination's options, and every new kind that takes commands would add a function there. |
| **The CLI keeping its own composition, tested to match** | The two compositions would drift, as the key sweep did. |

## Consequences

**A new store kind names what it needs, and every host that offers it
keeps records in it.** A new host names what it offers, and its stores,
its doctor layer and its line in the README follow.

**`herald housekeep` keeps a store's keyring bounded**, so a store that
keeps keys, as `[server.keys] cache = "store"` asks, no longer grows where
only the CLI keeps house.

## Related

- [ADR-0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md): the layers each host serves
- [ADR-0014](0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md): the server's rounds
- [ADR-0016](0016-commands-are-carried-every-way-a-destination-offers-and-answered-from-what-herald-keeps.md): the commands each destination is set up to take
- [ADR-0017](0017-herald-runs-wherever-code-runs-from-one-native-binary-and-one-wasm-module.md): the hosts and the composition, extended here
- [ADR-0018](0018-a-kind-is-data-and-an-event-carries-its-kind-s-attributes.md): kinds as data, which store kinds' needs join
