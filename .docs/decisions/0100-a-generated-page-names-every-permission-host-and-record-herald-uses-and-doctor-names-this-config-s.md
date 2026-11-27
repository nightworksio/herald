# ADR-0100: A generated page names every permission, host and record herald uses, and doctor names this config's

**Status:** Accepted
**Date:** 2026-11-27

## Context

What herald may do is written in many places: Slack's Web API methods and
their scopes are a table in `herald-dest-slack` from which the manifest is
built; Discord's permission bits, Matrix power levels and the forges'
read and write permissions appear in audience checks, probes and ADRs
(ADR-0068 decisions 3 and 4, ADR-0087 decision 13). Each destination
reaches only its own hosts (ADR-0007 decision 5). `deny.toml` bans
telemetry crates. No page collects any of it, and SECURITY.md states no
permission.

## Decision

1. **One declared table per kind.** `DestinationKind::PERMITS` and
   `SourceKind::PERMITS` are const tables of `Permit { feature,
   platform_permission, why, when }`; Slack's is built from its Web API
   table so the two cannot drift; the forges' write permissions are the
   consts the code mints narrowed tokens with and checks GitHub's answer
   against.
2. **A test per kind** fails where a request in that kind's recorded
   fixtures uses a method or permission its table does not name.
3. **`.docs/security.md` is generated** by
   `cargo run -p herald-compose --example security` and a test fails where
   the committed page differs, as for the settings reference. It holds,
   per kind: each permission and why; the hosts it calls, as patterns from
   `Reach::base` and each source's origin; what each request carries; per
   record kind, its fields, the stores that keep it, its retention key and
   default, and whether it is sealed; and a closed list of what herald
   never does, each line asserted by a named test (no telemetry crate, via
   cargo-deny; no request to an undeclared host, via the outbound fence
   tests; no secret in a file, via the wizard and import canary tests; no
   forge write without a `write` credential, via the loader's refusals).
   README's "Security" and SECURITY.md link it.
4. **`herald doctor --permissions`** prints the same for this config
   alone: each credential, the least permission each route and feature
   needs, and each host it reaches.

## Alternatives

| Option | Why not |
|--------|---------|
| **A hand-written page** | It would drift from the code the first time a call is added. |
| **Permissions in each destination's docs** | An adopter's security review wants one page. |

## Consequences

- An undeclared call fails CI before it ships.
- A reviewer reads one page for what herald may do, where it goes and
  what it keeps.

## Related

- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): hosts and the supply chain
- [ADR-0068](0068-an-operator-signed-in-at-the-forge-re-runs-a-planned-job-once.md): write permissions
- [ADR-0087](0087-a-pull-request-a-writer-queues-goes-to-its-forge-s-merge-queue-or-herald-s-own-proves-it-on-its-base-and-merges-it.md): merge queue permissions
- [ADR-0099](0099-herald-keeps-what-it-did-with-each-event-never-what-the-event-said-and-shows-it-to-the-operator-alone.md): the activity record's fields
