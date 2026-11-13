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
| [0001](0001-a-sans-io-core-behind-ports.md) | A sans-IO core behind ports, in crates that each name only those before them | Accepted; superseded in part by [0014](0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md) and [0017](0017-herald-runs-wherever-code-runs-from-one-native-binary-and-one-wasm-module.md) and [0023](0023-urls-are-read-one-way-in-the-core-text-is-escaped-one-way-and-a-registry-names-its-events-by-its-kind.md) |
| [0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md) | Three layers of support, each a superset of the one before, and a doctor that names what a layer cannot serve | Accepted |
| [0003](0003-every-source-becomes-one-neutral-event.md) | Every source becomes one neutral event, parsed without I/O | Accepted; superseded in part by [0018](0018-a-kind-is-data-and-an-event-carries-its-kind-s-attributes.md); extended by [0023](0023-urls-are-read-one-way-in-the-core-text-is-escaped-one-way-and-a-registry-names-its-events-by-its-kind.md) |
| [0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md) | Each destination gets its own dialect, limits and features, described as data | Accepted; superseded in part by [0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) |
| [0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md) | An announcement is sent once, finished if interrupted, and kept in any of nine stores | Accepted; superseded in part by [0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) and [0017](0017-herald-runs-wherever-code-runs-from-one-native-binary-and-one-wasm-module.md); extended by [0022](0022-a-read-back-that-fails-stops-the-step-and-pin-lookups-are-bounded.md) |
| [0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md) | One `herald.toml`, with sandboxed templates and a published JSON Schema, validated before anything posts | Accepted |
| [0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md) | Secrets stay out of argv, templates and logs, every inbound request is verified before it is parsed, and the supply chain is pinned and signed | Accepted |
| [0008](0008-every-line-is-covered-and-no-mutant-survives.md) | Every line is covered, no mutant survives, and every parser is fuzzed | Accepted |
| [0009](0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md) | herald ships as signed binaries, an Action, an image and a wasm build, from GitHub alone until 1.0 | Accepted; superseded in part by [0012](0012-a-release-is-drafted-by-the-bot-signed-by-the-maintainer-and-published-after-the-full-gate.md) |
| [0010](0010-herald-announces-itself-with-itself.md) | herald announces itself with itself | Accepted |
| [0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) | Markers count only on herald's own posts, a route keeps one pin, and a lock archives | Accepted; extended by [0022](0022-a-read-back-that-fails-stops-the-step-and-pin-lookups-are-bounded.md) |
| [0012](0012-a-release-is-drafted-by-the-bot-signed-by-the-maintainer-and-published-after-the-full-gate.md) | A release is drafted by the bot, signed by the maintainer, and published after the full gate | Accepted |
| [0013](0013-a-webhook-delivery-is-admitted-once-released-when-it-fails-and-kept-for-a-week.md) | A webhook delivery is admitted once, released when it fails, and kept for a week | Accepted |
| [0014](0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md) | herald server answers first and works after, on one task, within limits | Accepted; superseded in part by [0020](0020-an-activitypub-destination-sets-how-many-inboxes-it-delivers-to-at-once.md) |
| [0015](0015-an-app-is-a-source-routes-name-announcing-with-hashed-tokens-scoped-to-its-projects.md) | An app is a source routes name, announcing with hashed tokens scoped to its projects | Accepted; superseded in part by [0018](0018-a-kind-is-data-and-an-event-carries-its-kind-s-attributes.md) |
| [0016](0016-commands-are-carried-every-way-a-destination-offers-and-answered-from-what-herald-keeps.md) | Commands are carried every way a destination offers, and answered from what herald keeps | Accepted |
| [0017](0017-herald-runs-wherever-code-runs-from-one-native-binary-and-one-wasm-module.md) | herald runs wherever code runs, from one native binary and one wasm module | Accepted |
| [0018](0018-a-kind-is-data-and-an-event-carries-its-kind-s-attributes.md) | A kind is data, and an event carries its kind's attributes | Accepted; extended by [0019](0019-a-kind-s-links-are-its-buttons-each-labelled-by-the-config-or-by-herald.md) |
| [0019](0019-a-kind-s-links-are-its-buttons-each-labelled-by-the-config-or-by-herald.md) | A kind's links are its buttons, each labelled by the config or by herald | Accepted; extended by [0021](0021-a-kind-names-at-most-24-links-each-fitting-a-button.md) |
| [0020](0020-an-activitypub-destination-sets-how-many-inboxes-it-delivers-to-at-once.md) | An ActivityPub destination sets how many inboxes it delivers to at once | Accepted |
| [0021](0021-a-kind-names-at-most-24-links-each-fitting-a-button.md) | A kind names at most 24 links, each fitting a button | Accepted |
| [0022](0022-a-read-back-that-fails-stops-the-step-and-pin-lookups-are-bounded.md) | A read-back that fails stops the step, and pin lookups are bounded | Accepted |
| [0023](0023-urls-are-read-one-way-in-the-core-text-is-escaped-one-way-and-a-registry-names-its-events-by-its-kind.md) | URLs are read one way in the core, text is escaped one way, and a registry names its events by its kind | Accepted |
