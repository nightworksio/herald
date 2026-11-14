# ADR-0027: The layer rules place every crate, know every runtime, and ship no fakes

**Status:** Accepted
**Date:** 2026-11-14

## Context

ADR-0001 orders the workspace in layers, and `herald-arch` holds that
order. Its kits row names `herald-sql`, a crate that no longer exists: the
SQL the SQL stores run is `herald-records`' own. The kits layer now also
holds the kinds' registry, the keys' encodings and AWS signing. The rules
allowed `ring` in the kits, though no kit names it and the runtime rule
refuses what it reaches. They also knew only some of the runtime and I/O
crates. A crate could name `herald-testing` as a normal dependency, since
the testing layer comes before every crate that would ship it. And the bans
on ambient I/O, together with the lints, left gaps: part of the filesystem
and the environment, ending the process, resolving names, reading elapsed
time, and printing.

## Decision

1. **The kits are `herald-aws`, `herald-keys`, `herald-kinds`,
   `herald-records` and `herald-wire`**, and they may name `base64`, `hmac`,
   `jiff`, `serde`, `serde_json` and `sha2` from outside the workspace.
2. **Every workspace crate has a place.** The placement test lists each one,
   and a crate no layer claims is a violation.
3. **No crate ships the fakes.** Naming `herald-testing` as a normal
   dependency is a violation unless the dependency is optional, turned on by
   a feature such as `conformance`.
4. **Before the I/O layers, no crate reaches a runtime, a network client, a
   database client, a platform binding or a source of randomness.** The list
   covers every such crate the workspace resolves, `ring` among them.
5. **Before the I/O layers, clippy bans every ambient read or write of the
   filesystem and the environment, ending the process, resolving names and
   reading elapsed time.** `dbg!` is denied everywhere. Printing to
   standard output or standard error is denied everywhere except where an
   I/O layer's log or a schema tool says it expects it.

## Consequences

**A new crate must be placed before it builds**, and a new runtime crate
that a core crate reaches is caught by the rule that names it.

**A conformance build still carries the suites**, behind its feature.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): the layers and their rules, its kits row superseded here
- [ADR-0008](0008-every-line-is-covered-and-no-mutant-survives.md): the gates `herald-arch` runs among
- [ADR-0017](0017-herald-runs-wherever-code-runs-from-one-native-binary-and-one-wasm-module.md): the composition layer and the drivers it added
