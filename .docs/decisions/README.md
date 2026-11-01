# Architecture Decision Records

An ADR records a decision where a reasonable engineer could have chosen
otherwise. It gives the alternatives and why they lost, so that revisiting the
decision starts from evidence rather than from scratch. Together with the
[README](../../README.md), the ADRs are the whole of herald's design. There is
no separate requirements document, and there are no requirement IDs.

## Rules

1. **An ADR is immutable once Accepted.** To change a decision, write a new
   ADR that supersedes the old one, and link the two both ways.
2. **Numbers are permanent and never reused.**
3. **A pull request that implements a decision names it** in its body, so
   the squash commit on `main` carries the link.

## Index

| # | Decision | Status |
|---|----------|--------|
| [0001](0001-a-sans-io-core-behind-ports.md) | A sans-IO core behind ports, in crates that each name only those before them | Accepted |
| [0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md) | Three layers of support, each a superset of the one before, and a doctor that names what a layer cannot serve | Accepted |
| [0003](0003-every-source-becomes-one-neutral-event.md) | Every source becomes one neutral event, parsed without I/O | Accepted |
| [0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md) | Each destination gets its own dialect, limits and features, described as data | Accepted; superseded in part by [0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) |
| [0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md) | An announcement is sent once, finished if interrupted, and kept in any of nine stores | Accepted; superseded in part by [0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) |
| [0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md) | One `herald.toml`, with sandboxed templates and a published JSON Schema, validated before anything posts | Accepted |
| [0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md) | Secrets stay out of argv, templates and logs, every inbound request is verified before it is parsed, and the supply chain is pinned and signed | Accepted |
| [0008](0008-every-line-is-covered-and-no-mutant-survives.md) | Every line is covered, no mutant survives, and every parser is fuzzed | Accepted |
| [0009](0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md) | herald ships as signed binaries, an Action, an image and a wasm build, from GitHub alone until 1.0 | Accepted; superseded in part by [0012](0012-a-release-is-drafted-by-the-bot-signed-by-the-maintainer-and-published-after-the-full-gate.md) |
| [0010](0010-herald-announces-itself-with-itself.md) | herald announces itself with itself | Accepted |
| [0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) | Markers count only on herald's own posts, a route keeps one pin, and a lock archives | Accepted |
| [0012](0012-a-release-is-drafted-by-the-bot-signed-by-the-maintainer-and-published-after-the-full-gate.md) | A release is drafted by the bot, signed by the maintainer, and published after the full gate | Accepted |
