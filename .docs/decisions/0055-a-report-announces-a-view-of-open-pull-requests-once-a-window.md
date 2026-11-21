# ADR-0055: A report announces a view of open pull requests once a window

**Status:** Accepted; `herald queue` withholding private repositories' rows too extended by [ADR-0069](0069-herald-s-commands-withhold-private-repositories-rows-unless-their-output-is-said-private.md)
**Date:** 2026-11-18

## Context

A maintainer's queue is state, not an event: which pull requests are open,
whose turn each is, and how long each has waited. The maintainer dashboard
(step 13d) shows it on every surface from one definition, starting with a
digest posted to chat and a terminal view. herald already announces
events once through routes (ADR-0005), and apps share the route source
namespace (ADR-0015); the `pull-request` kind (ADR-0052) carries a pull
request's lifecycle, not its reviews or checks.

## Decision

1. **Views.** `[views.<name>]` names the sources it reads (each naming one
   repository on a forge), the states it shows, what it does with bots
   (`include`, `exclude`, `only`, `batch`), globs over repositories,
   authors and labels, `stale` (14 days), `stale_only`, `group_by`
   (`turn`, `repository`, `author`, `nothing`), what decides whose turn it
   is, herald's rules each switchable, its own bump patterns, `most_open`
   (500) and `most_pulls` (100). A view named `queue`, over every such
   source, is built in, and `[views.queue]` replaces it.
2. **Whose turn.** By herald's rules (draft, queued, conflict, changes
   requested, a failed check, pending checks, approval), by the forge's
   own status, or by the first of an ordered list of labels. Every row
   carries its reason.
3. **Bots batch by bump.** Dependabot's and Renovate's titles, and a
   view's own `{package}`/`{to}` patterns read first, gather the same bump
   across repositories, with how many are green and how many failing.
4. **Reads, by forge and credential.** GitHub with a token reads GraphQL;
   GitLab its merge request listing; GitHub without a token and Forgejo a
   REST listing, then each pull request's reviews and its head's checks,
   up to `most_pulls`, past which a pull request reads `unknown`. GitHub's
   REST listing names no mergeability, so its batches count none green. A
   REST review decides only where GitHub names its author an owner,
   member or collaborator, or Forgejo marks it official.
5. **Reports are sources.** `[reports.<name>]` names a view and when its
   windows begin: `every` (1m to 1d, default 1d), `at` ("HH:MM", default
   09:00), `days`, and an IANA `zone` (default UTC); a route names the
   report among its sources, sharing one namespace with sources and apps.
   Each poll, after its sources, and `POST /herald/round/report` announce
   each report whose window began as a `report` event, identity
   `report/<name>/<window start>`, attributes `view`, `pulls` and `stale`.
   The report's cursor remembers the window once no route stopped. Only
   herald announces a `report`: apps and peers' feeds give none.
6. **Templates read `view`.** The context gains `view`, built through an
   allowlist a test pins, and herald ships `herald:queue`, which a
   `[templates."herald:queue"]` table replaces. `herald queue [view]
   --format table|json` prints a view now; its JSON holds the same keys.
7. **Private repositories fail closed.** A repository of unknown
   visibility counts as private. Where a view read a repository not
   everyone may see and `[doctor] private_to_public` refuses, private
   routes are shown every row and the others only the public
   repositories' rows, or nothing where none was read.
8. **Doctor** names each report's read path per source, the most requests
   a round takes, the rows past `most_pulls`, the missing mergeability,
   the bundled time zone database, and which routes see which rows.
9. **Time zones are bundled.** `herald-core`'s default `tzdb` feature
   bundles the database (jiff-tzdb: 207,040 bytes, 48,924 gzipped). A
   build without it reads only UTC.

## Consequences

- A digest of open pull requests reaches every destination from
  `herald.toml` alone, once a window, on every host.
- A view reads its forge live each window: on GitHub without a token or on
  Forgejo, up to two requests per pull request read in full.
- The Workers wasm build grew from 23,357,722 to 24,244,662 bytes
  (5,383,307 to 5,605,409 gzipped) with this change; the time zone
  database is 207,040 of those bytes (48,924 gzipped). Every crate takes
  `herald-core` with its default features, so a build without the
  database would set the workspace dependency's `default-features =
  false`; at this size no host needs it.
- Owner sources, kept records, nudges and pages come in later phases.
