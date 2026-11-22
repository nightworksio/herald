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
| [0001](0001-a-sans-io-core-behind-ports.md) | A sans-IO core behind ports, in crates that each name only those before them | Accepted; superseded in part by [0014](0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md) and [0017](0017-herald-runs-wherever-code-runs-from-one-native-binary-and-one-wasm-module.md) and [0023](0023-urls-are-read-one-way-in-the-core-text-is-escaped-one-way-and-a-registry-names-its-events-by-its-kind.md); kits and layer rules superseded by [0027](0027-the-layer-rules-place-every-crate-and-ship-no-fakes.md) |
| [0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md) | Three layers of support, each a superset of the one before, and a doctor that names what a layer cannot serve | Accepted |
| [0003](0003-every-source-becomes-one-neutral-event.md) | Every source becomes one neutral event, parsed without I/O | Accepted; superseded in part by [0018](0018-a-kind-is-data-and-an-event-carries-its-kind-s-attributes.md); extended by [0023](0023-urls-are-read-one-way-in-the-core-text-is-escaped-one-way-and-a-registry-names-its-events-by-its-kind.md), [0029](0029-an-owner-source-polls-each-repository-it-takes-on-its-own.md) and [0031](0031-a-source-announces-releases-tags-or-both-and-tells-a-pre-release-by-its-own-rule.md) |
| [0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md) | Each destination gets its own dialect, limits and features, described as data | Accepted; superseded in part by [0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) |
| [0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md) | An announcement is sent once, finished if interrupted, and kept in any of nine stores | Accepted; superseded in part by [0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) and [0017](0017-herald-runs-wherever-code-runs-from-one-native-binary-and-one-wasm-module.md); extended by [0022](0022-a-read-back-that-fails-stops-the-step-and-pin-lookups-are-bounded.md), [0025](0025-routes-and-sources-go-at-once-within-limits-and-a-session-keeps-where-it-stands.md) and [0026](0026-a-store-s-write-attempts-lock-and-tombstone-are-settings.md) |
| [0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md) | One `herald.toml`, with sandboxed templates and a published JSON Schema, validated before anything posts | Accepted |
| [0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md) | Secrets stay out of argv, templates and logs, every inbound request is verified before it is parsed, and the supply chain is pinned and signed | Accepted |
| [0008](0008-every-line-is-covered-and-no-mutant-survives.md) | Every line is covered, no mutant survives, and every parser is fuzzed | Accepted |
| [0009](0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md) | herald ships as signed binaries, an Action, an image and a wasm build, from GitHub alone until 1.0 | Accepted; superseded in part by [0012](0012-a-release-is-drafted-by-the-bot-signed-by-the-maintainer-and-published-after-the-full-gate.md) |
| [0010](0010-herald-announces-itself-with-itself.md) | herald announces itself with itself | Accepted |
| [0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md) | Markers count only on herald's own posts, a route keeps one pin, and a lock archives | Accepted; extended by [0022](0022-a-read-back-that-fails-stops-the-step-and-pin-lookups-are-bounded.md) |
| [0012](0012-a-release-is-drafted-by-the-bot-signed-by-the-maintainer-and-published-after-the-full-gate.md) | A release is drafted by the bot, signed by the maintainer, and published after the full gate | Accepted |
| [0013](0013-a-webhook-delivery-is-admitted-once-released-when-it-fails-and-kept-for-a-week.md) | A webhook delivery is admitted once, released when it fails, and kept for a week | Accepted |
| [0014](0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md) | herald server answers first and works after, on one task, within limits | Accepted; superseded in part by [0020](0020-an-activitypub-destination-sets-how-many-inboxes-it-delivers-to-at-once.md) |
| [0015](0015-an-app-is-a-source-routes-name-announcing-with-hashed-tokens-scoped-to-its-projects.md) | An app is a source routes name, announcing with hashed tokens scoped to its projects | Accepted; superseded in part by [0018](0018-a-kind-is-data-and-an-event-carries-its-kind-s-attributes.md); extended by [0064](0064-a-ci-run-s-id-token-is-its-credential.md) |
| [0016](0016-commands-are-carried-every-way-a-destination-offers-and-answered-from-what-herald-keeps.md) | Commands are carried every way a destination offers, and answered from what herald keeps | Accepted; extended by [0025](0025-routes-and-sources-go-at-once-within-limits-and-a-session-keeps-where-it-stands.md) |
| [0017](0017-herald-runs-wherever-code-runs-from-one-native-binary-and-one-wasm-module.md) | herald runs wherever code runs, from one native binary and one wasm module | Accepted; extended by [0024](0024-a-host-is-described-once-and-every-driver-composes-through-one-composition.md) |
| [0018](0018-a-kind-is-data-and-an-event-carries-its-kind-s-attributes.md) | A kind is data, and an event carries its kind's attributes | Accepted; extended by [0019](0019-a-kind-s-links-are-its-buttons-each-labelled-by-the-config-or-by-herald.md), [0051](0051-a-forge-source-announces-its-security-alerts-to-private-places.md) |
| [0019](0019-a-kind-s-links-are-its-buttons-each-labelled-by-the-config-or-by-herald.md) | A kind's links are its buttons, each labelled by the config or by herald | Accepted; extended by [0021](0021-a-kind-names-at-most-24-links-each-fitting-a-button.md) |
| [0020](0020-an-activitypub-destination-sets-how-many-inboxes-it-delivers-to-at-once.md) | An ActivityPub destination sets how many inboxes it delivers to at once | Accepted |
| [0021](0021-a-kind-names-at-most-24-links-each-fitting-a-button.md) | A kind names at most 24 links, each fitting a button | Accepted |
| [0022](0022-a-read-back-that-fails-stops-the-step-and-pin-lookups-are-bounded.md) | A read-back that fails stops the step, and pin lookups are bounded | Accepted |
| [0023](0023-urls-are-read-one-way-in-the-core-text-is-escaped-one-way-and-a-registry-names-its-events-by-its-kind.md) | URLs are read one way in the core, text is escaped one way, and a registry names its events by its kind | Accepted |
| [0024](0024-a-host-is-described-once-and-every-driver-composes-through-one-composition.md) | A host is described once, and every driver composes through one composition | Accepted |
| [0025](0025-routes-and-sources-go-at-once-within-limits-and-a-session-keeps-where-it-stands.md) | Routes and sources go at once within limits, and a session keeps where it stands | Accepted |
| [0026](0026-a-store-s-write-attempts-lock-and-tombstone-are-settings.md) | A store's write attempts, lock and tombstone are settings | Accepted |
| [0027](0027-the-layer-rules-place-every-crate-and-ship-no-fakes.md) | The layer rules place every crate, know every runtime, and ship no fakes | Accepted |
| [0028](0028-work-keeps-within-its-host-s-limits-and-starts-no-send-it-cannot-finish.md) | Work keeps within its host's limits and starts no send it cannot finish | Accepted |
| [0029](0029-an-owner-source-polls-each-repository-it-takes-on-its-own.md) | An owner source polls each repository it takes on its own | Accepted; extended by [0030](0030-an-owner-source-reads-a-member-file-takes-webhooks-names-projects-and-polls-in-batches.md) |
| [0030](0030-an-owner-source-reads-a-member-file-takes-webhooks-names-projects-and-polls-in-batches.md) | An owner source reads a member file, takes webhooks, names projects and polls in batches | Accepted; extended by [0031](0031-a-source-announces-releases-tags-or-both-and-tells-a-pre-release-by-its-own-rule.md) |
| [0031](0031-a-source-announces-releases-tags-or-both-and-tells-a-pre-release-by-its-own-rule.md) | A source announces releases, tags or both, and tells a pre-release by its own rule | Accepted; extended and superseded in part by [0032](0032-a-tag-waits-for-its-release-in-a-held-record-for-its-window.md); extended by [0034](0034-a-post-is-edited-to-a-late-or-edited-release.md) and [0035](0035-a-source-announces-the-versions-of-its-repositories-packages.md) |
| [0032](0032-a-tag-waits-for-its-release-in-a-held-record-for-its-window.md) | A tag waits for its Release in a held record, for its window | Accepted; extended by [0033](0033-a-ci-run-for-a-tag-announces-the-tag-or-holds-it.md) |
| [0033](0033-a-ci-run-for-a-tag-announces-the-tag-or-holds-it.md) | A CI run for a tag announces the tag, or holds it | Accepted |
| [0034](0034-a-post-is-edited-to-a-late-or-edited-release.md) | A post is edited to a late or edited release | Accepted |
| [0035](0035-a-source-announces-the-versions-of-its-repositories-packages.md) | A source announces the versions of its repositories' packages | Accepted |
| [0036](0036-a-route-announces-each-project-as-an-actor-of-its-own.md) | A route announces each project as an actor of its own | Accepted |
| [0037](0037-gitlab-owners-poll-through-graphql-and-forgejo-owners-follow-their-feed.md) | GitLab owners poll through GraphQL, and Forgejo owners follow their feed | Accepted |
| [0038](0038-a-route-matches-each-event-by-its-origin.md) | A route matches each event by its origin | Accepted; extended by [0050](0050-a-forge-source-announces-its-ci-runs-and-a-route-matches-by-attributes.md) |
| [0039](0039-a-registry-source-takes-every-package-of-a-namespace.md) | A registry source takes every package of a namespace | Accepted |
| [0040](0040-a-ghcr-source-announces-an-owner-s-images-as-their-repositories-projects.md) | A ghcr source announces an owner's images as their repositories' projects | Accepted |
| [0041](0041-an-owner-learns-of-its-repositories-by-push-and-reads-as-a-github-app.md) | An owner learns of its repositories by push, and reads as a GitHub App | Accepted |
| [0042](0042-an-oci-source-announces-an-image-s-tags-on-any-registry.md) | An oci source announces an image's tags on any registry | Accepted |
| [0043](0043-instances-sign-with-ed25519-prove-their-objects-and-publish-a-key-set.md) | Instances sign with Ed25519, prove their objects, and publish a key set | Accepted |
| [0044](0044-an-instance-is-found-by-its-discovery-document-and-federation-holds-its-identity.md) | An instance is found by its discovery document, and `[federation]` holds its identity | Accepted; extended by [0046](0046-a-herald-feed-publishes-each-public-event-as-a-proved-envelope-and-the-api-signs-every-answer.md), [0047](0047-a-peer-is-pinned-by-the-keys-its-operator-confirms.md) |
| [0045](0045-a-route-gathers-the-events-that-share-a-key-into-one-announcement.md) | A route gathers the events that share a key into one announcement | Accepted; extended by [0048](0048-a-release-the-forge-deletes-is-withdrawn-from-each-route-that-asks.md) |
| [0046](0046-a-herald-feed-publishes-each-public-event-as-a-proved-envelope-and-the-api-signs-every-answer.md) | A herald feed publishes each public event as a proved envelope, and the API signs every answer | Accepted |
| [0047](0047-a-peer-is-pinned-by-the-keys-its-operator-confirms.md) | A peer is pinned by the keys its operator confirms | Accepted; extended by [0049](0049-a-herald-source-pulls-a-peer-s-feed-through-the-keys-that-stand-for-it.md) |
| [0048](0048-a-release-the-forge-deletes-is-withdrawn-from-each-route-that-asks.md) | A release the forge deletes is withdrawn from each route that asks | Accepted; extended by [0049](0049-a-herald-source-pulls-a-peer-s-feed-through-the-keys-that-stand-for-it.md) |
| [0049](0049-a-herald-source-pulls-a-peer-s-feed-through-the-keys-that-stand-for-it.md) | A herald source pulls a peer's feed through the keys that stand for it | Accepted; extended by [0054](0054-a-peer-pushes-into-a-herald-source-it-is-granted.md) |
| [0050](0050-a-forge-source-announces-its-ci-runs-and-a-route-matches-by-attributes.md) | A forge source announces its CI runs, and a route matches by attributes | Accepted |
| [0051](0051-a-forge-source-announces-its-security-alerts-to-private-places.md) | A forge source announces its security alerts to private places | Accepted |
| [0052](0052-a-forge-source-announces-its-pull-requests.md) | A forge source announces its pull requests | Accepted |
| [0053](0053-a-forge-source-announces-its-issues.md) | A forge source announces its issues | Accepted |
| [0054](0054-a-peer-pushes-into-a-herald-source-it-is-granted.md) | A peer pushes into a herald source it is granted | Accepted; extended by [0064](0064-a-ci-run-s-id-token-is-its-credential.md) |
| [0055](0055-a-report-announces-a-view-of-open-pull-requests-once-a-window.md) | A report announces a view of open pull requests once a window | Accepted |
| [0056](0056-a-board-keeps-one-post-of-a-report-and-a-rule-nudges-each-pull-request-past-its-stages.md) | A board keeps one post of a report, and a rule nudges each pull request past its stages | Accepted |
| [0057](0057-a-view-asks-a-model-only-what-its-rules-cannot-tell-and-keeps-every-answer.md) | A view asks a model only what its rules cannot tell, and keeps every answer | Accepted |
| [0058](0058-doctor-shows-the-instance-among-other-heralds.md) | doctor shows the instance among other heralds | Accepted |
| [0059](0059-a-model-is-asked-through-its-provider-s-own-api-or-the-binding-of-the-host-that-runs-it.md) | A model is asked through its provider's own API, or the binding of the host that runs it | Accepted |
| [0060](0060-a-static-host-serves-the-feeds-as-a-signed-log.md) | A static host serves the feeds as a signed log | Accepted |
| [0061](0061-a-view-reads-kept-pull-requests-folded-from-webhooks-and-reconciled-with-a-listing.md) | A view reads kept pull requests, folded from webhooks and reconciled with a listing | Accepted |
| [0062](0062-a-view-reads-an-owner-s-repositories-by-one-query-over-them.md) | A view reads an owner's repositories by one query over them | Accepted |
| [0063](0063-a-view-predicts-conflicts-from-the-paths-open-pull-requests-change.md) | A view predicts conflicts from the paths open pull requests change | Accepted |
| [0064](0064-a-ci-run-s-id-token-is-its-credential.md) | A CI run's ID token is its credential | Accepted |
| [0065](0065-a-view-triages-failing-checks-and-a-rule-plans-a-re-run-of-a-flaky-one.md) | A view triages failing checks, and a rule plans a re-run of a flaky one | Accepted |
| [0066](0066-a-view-shows-its-repositories-ci-capacity.md) | A view shows its repositories' CI capacity | Accepted |
| [0067](0067-a-view-shows-its-repositories-advisories-to-private-routes-alone.md) | A view shows its repositories' advisories to private routes alone | Accepted |
| [0068](0068-an-operator-signed-in-at-the-forge-re-runs-a-planned-job-once.md) | An operator signed in at the forge re-runs a planned job once | Accepted |
| [0069](0069-herald-s-commands-withhold-private-repositories-rows-unless-their-output-is-said-private.md) | herald's commands withhold private repositories' rows unless their output is said private | Accepted |
| [0070](0070-the-nightly-run-mutates-what-changed-and-the-weekly-run-everything.md) | The nightly run mutates what changed, and the weekly run everything | Accepted |
| [0071](0071-a-source-names-its-repositories-areas-by-path-globs.md) | A source names its repositories' areas by path globs | Accepted |
| [0072](0072-a-route-narrows-pull-request-events-by-the-areas-their-paths-fall-in.md) | A route narrows pull request events by the areas their paths fall in | Accepted |
