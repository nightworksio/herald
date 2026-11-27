# ADR-0095: Preview renders every kind as its destination sends it, reading nothing, and a gallery is generated from it

**Status:** Accepted
**Date:** 2026-11-27

## Context

`herald preview` renders each route's templates as its destination would
show them, resolving no secret and sending nothing; the dialect and limits
come from the config and the markers are placeholders. It renders release
events alone: the config's release samples, or a release event given with
`--event` and `--from`. Its output is herald's neutral announcement (title,
thread, each part's body in the dialect), not the request a destination
sends. `herald announce --dry-run` prints every request a first
announcement sends, secrets redacted, but needs a real run event and reads
the secrets of what the run does. herald's docs are the repository's
Markdown; it has no website.

## Decision

1. **Every kind.** `--kind <kind>` renders a kind's samples; without it,
   every kind a route takes. `--event <file>` takes any payload a forge
   sends that a source maps, `-` reading stdin; `--sample <name>` picks a
   shipped sample payload; `--report <name>` renders a report's template
   against a sample view built from fixtures.
2. **Three formats.** `--format text` (the default, today's output);
   `wire`, the exact request body each destination's own request builder
   writes (Block Kit, embed, Matrix event, Adaptive Card, MIME message),
   each secret as `«secret:NAME»` and each URL as the configured base;
   `json`, for tools and the gallery.
3. **No config needed.** `--destination <kind>` previews a sample against
   that kind's default profile.
4. **Preview reads nothing.** It resolves no secret and opens no
   connection on any path; a test runs it with ports that fail the test
   when called. Private repositories' events print as `(private)` unless
   `--private` (ADR-0080). Reading the forge stays with
   `announce --dry-run`.
5. **The gallery is generated.** For each destination, kind and sample,
   an entry shows the message as text, the destination's own format (the
   `wire` output) and a drawing of how it looks: an SVG approximation
   drawn from the wire output by herald's own code, holding no script, no
   remote resource and no font but the reader's. Each entry is generated
   from the golden files the destinations' tests assert, by
   `cargo run -p herald-compose --example gallery`, into `.docs/gallery/`,
   and a test fails where the committed pages differ from what it writes,
   as for the settings reference.
6. **The gallery is published twice.** As the Markdown pages in the
   repository, and as a GitHub Pages site built from those same pages by a
   workflow: a build job on every pull request that touches them, and a
   deploy job on `main` through GitHub's own Pages actions, with
   `pages: write` and `id-token: write` on that job alone, which runs
   only where the repository variable `HERALD_PAGES` is set, so `main`
   stays green until Pages is switched on. The site holds
   only generated pages and herald's own stylesheet: no script, no
   analytics, no third-party asset.

## Alternatives

| Option | Why not |
|--------|---------|
| **A new command beside preview** | Preview already renders as the destination would; a second command would render a second way. |
| **Screenshots from test workspaces** | Taken by hand, stale after any change. |
| **Text and platform format only** | A newcomer judging a chat wants to see the shape of the message. |
| **Markdown in the repository only** | A site is where a newcomer looks first. |
| **The site from its own sources** | Two sources would drift; the site is built from the tested pages. |

## Consequences

- Every kind can be seen before anything posts, on every destination.
- A change to a destination's output changes its golden files, its
  gallery entry and the site in one pull request.
- The site is served once the repository's Pages source is set to GitHub
  Actions, a repository setting.

## Related

- [ADR-0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md): validated before anything posts
- [ADR-0045](0045-a-route-gathers-the-events-that-share-a-key-into-one-announcement.md): `herald preview --group`
- [ADR-0080](0080-a-poll-announces-a-milestone-overdue-by-its-own-clock-and-poll-and-announce-name-private-events-where-said-private.md): private events named "(private)"
- [ADR-0091](0091-herald-init-writes-a-first-config-and-its-workflow-and-posts-one-test-message.md): the test post previews first
