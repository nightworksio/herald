# ADR-0003: Every source becomes one neutral event, parsed without I/O

**Status:** Accepted
**Date:** 2026-10-31

## Context

herald announces four families of things:

- **forge releases** on GitHub, GitLab and Forgejo or Gitea;
- **package versions** on Packagist, crates.io, npm and Docker Hub;
- **feed entries** from RSS and Atom;
- **anything an app says**, through the CLI or the HTTP API.

Each family arrives in its own shape: a webhook payload, a CI's event file, a
registry's JSON, or a feed's XML. Routes, templates and the never-twice key
must not care which shape an announcement came from. A template written for a
GitHub release should render a Forgejo release unchanged.

## Decision

1. **Every source produces an `Event`, and nothing else reaches the core.**
   An `Event` carries:

   | Field | Holds |
   |-------|-------|
   | `identity` | What makes this event this event, stable across reruns and polls (decision 3) |
   | `kind` | `release`, `package-version`, `feed-entry` or `message` |
   | `project` | The project's name, as the route and the template read it |
   | `version` | The version, where the kind has one |
   | `title` | A one-line title |
   | `body` | Markdown, converted from the source's own format |
   | `links` | Named links: the release page, the changelog, the comparison, the download |
   | `prerelease` | Whether this is a pre-release, read from the source's own flag or the version's pre-release part |
   | `occurred` | When it happened, as the source says |

2. **One crate per family, each a pure parser.**
   - **`herald-sources-forge`** parses each forge's release webhook, each CI's
     own event, and each forge's releases API:
     - GitHub's `release` event and GitHub Actions' event file;
     - GitLab's release hook and a GitLab CI tag pipeline;
     - Forgejo's and Gitea's `release` event and Forgejo Actions' event file.

     A release event announces on `published` only, and a draft never
     announces.
   - **`herald-sources-registry`** parses Packagist's `p2` metadata, crates.io's
     versions API, the npm registry's packument, and Docker Hub's tags API. A
     Docker Hub tag is announced when it matches the route's tag pattern, so
     moving tags such as `latest` stay quiet.
   - **`herald-sources-feed`** parses RSS 2.0 and Atom 1.0. An entry's HTML is
     sanitised and converted to Markdown.
   - **`herald-sources-app`** parses herald's own event JSON, published as a
     JSON Schema beside the config's (ADR-0006). The CLI reads it from a file
     or standard input, and the HTTP API from a request body.

   Where a source must ask the outside world, as polling must, it returns the
   description of the request. The flow sends that request through `Http`, and
   the source parses the response it is handed back (ADR-0001). Every request
   carries a `User-Agent` naming herald, its version and its repository, as
   crates.io requires.

3. **Identity is stable under the renames that happen.**

   | Source | Identity |
   |--------|----------|
   | GitHub, GitLab, Forgejo | the forge's host, the repository's numeric id, and the tag |
   | Packagist, crates.io, npm | the registry, the package, and the version |
   | Docker Hub | the repository and the tag |
   | RSS, Atom | the feed's URL, and the entry's `guid` or `id`. Without either, its link. Without a link, a digest of its title and date |
   | An app | the app's name, from its token or the CLI's config, and the `id` the app gives |

   A repository renamed on its forge keeps its numeric id, so its earlier
   releases are not announced again.

4. **Polling remembers what it has seen, and starts quiet.** Each polled
   source keeps a `Cursor` in the store: the newest identities seen, and the
   validators the server returned (`ETag`, `Last-Modified`). Each request is
   conditional, so an unchanged source costs one `304`. The first poll of a
   new source records what is there and announces nothing, so adding a feed
   with ten years of entries does not post ten years of entries. `backfill =
   N` on the source announces its newest *N* on that first poll instead.

5. **How each family reaches herald, by layer** (ADR-0002):
   - **Forge releases.** On Actions, `herald announce --from github` reads the
     run's event, as do `--from gitlab` on GitLab CI and `--from forgejo` on
     Forgejo Actions. At layer two and up, a verified webhook reaches the
     endpoint (ADR-0007). At every layer, a scheduled poll of the releases
     API reaches repositories that are not the workflow's own.
   - **Registries and feeds** are polled on a schedule at every layer.
   - **An app** announces with `herald announce --event` at every layer, and
     through the HTTP API at layer two and up.

6. **Parsers are bounded and refuse what they cannot read.** Each parser caps
   the size it reads. The XML parser expands no entity it does not know.
   Malformed input becomes an error naming the source and the field, never a
   panic. Every parser is fuzzed (ADR-0008).

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **Templates written against each source's payload** | Every template would have to know whether it was reading a GitHub or a GitLab payload, and a route could not take two sources. |
| **One crate per source** (a crate for GitHub, another for GitLab) | Within a family the parsers share most of their shape: three forges' releases and four registries' version lists. Splitting them would repeat that shared part, or move it into the core, where it does not belong. |
| **Identity by repository name** | A rename on the forge would make every earlier release new again, and a poll would announce them all. |
| **Announce everything on the first poll** | Adding a source would flood the channel with its history. Starting quiet, with an explicit `backfill`, makes the flood opt-in. |
| **Registry webhooks** | Docker Hub's are unsigned, and Packagist, crates.io and npm offer none to third parties. A conditional poll costs one `304` when nothing changed, and works at every layer. |

## Consequences

**A route can take several sources.** "Every release of these three
repositories, on any forge" is one route with one template.

**Adding a source is a parser and its fixtures.** The core, the routes and
the destinations do not change.

**The parsers are the inbound attack surface, and they hold no I/O.** That
makes them the code fuzzing reaches best.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): sources as pure parsers
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the key built from the identity
- [ADR-0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md): the event schema and what templates read
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): verifying webhooks before parsing them
