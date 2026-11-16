# ADR-0039: A registry source takes every package of a namespace

**Status:** Accepted
**Date:** 2026-11-16

## Context

A registry source names one package (ADR-0003). A project that publishes
many packages under an npm scope, a Packagist vendor, a crates.io owner or
a Docker Hub namespace needed a source for each, and a new package was
announced only once someone added its source. Forge owner sources already
expand to every repository an owner holds (ADR-0029).

## Decision

1. **A namespace key replaces `package`.** npm takes `scope` (`@acme`),
   Packagist `vendor`, crates.io `owner` (`user:<login>` or
   `team:github:<org>:<team>`, as crates.io writes owners) and Docker Hub
   `namespace`. A source gives `package` or its registry's namespace key,
   never both, and no other registry's key.
2. **A namespace is listed once a round, and each package polled on its
   own**, as an owner's members are: through the same `Expand` step, with
   per-member cursors, `keep_members` ("30d") for a package that leaves,
   and `most_listed` (10000) and `most_members` (1000, at most 100000)
   refusing a round over either. `include` and `exclude` globs choose
   among the packages. A member's identity is a single-package source's,
   so the two share it.
3. **Each registry lists as it can.** npm: `/-/org/<scope>/package`,
   taking `@scope/*` packages, and the organization's unscoped ones too
   with `unscoped = true`. A refused listing fails, keeping the packages
   last listed, and says to give a token or name packages one by one; npm's search is no fallback,
   as its scope qualifier no longer narrows. Packagist: `/packages/list.json`
   at `listing_api` (`https://packagist.org`), or, with
   `listing = "composer"`, a Composer repository's `packages.json`
   `available-packages` at `api`. crates.io: the owner's id, then its crates
   page by page. Docker Hub: the namespace's repositories page by page,
   after the token exchange. `listed_page` sets a page where the registry
   pages its listing (100, at most 100). A listing is confined to its
   namespace: a Packagist vendor listing or a Docker Hub page naming a
   package of another vendor or namespace fails, naming it, and doctor
   names the gap at the namespace key; npm and a Composer repository keep
   only the namespace's packages, as their answers hold others. Each name
   a listing gives is checked as a configured `package` is, and a page
   naming one that is not a package's name fails, naming it, before any
   request carries it.
4. **A source may reach more than one origin.** A `Poll` or `Expand` names
   its `bases`, and its connection reaches each of their origins
   (`Base::Among`) and no other (ADR-0007). A Packagist vendor reaches its
   registry and `listing_api`; its token goes only to the registry.
5. **A package's visibility is the listing's where it says, and fails
   closed where it does not.** Docker Hub names `is_private`. Elsewhere a
   package is public where the source has no token, and private where a
   token may list private packages, unless `visibility = "public"`
   declares the namespace public. `[doctor] private_to_public` then
   refuses a token-bearing namespace on a public route until it is
   declared public.
6. **Docker Hub's identities take the registry's shape,**
   `docker.io/<namespace>/<repository>/<tag>`, with official images under
   `library/`, so they equal a forge's container identity (ADR-0035) and
   any other source of the image.

## Consequences

- A scope, vendor, owner or namespace announces a new package from its
  first version, with no change to the config.
- A namespace of N packages costs a listing and N requests a round; each
  package is asked with its own `ETag`, and a listing in one answer is
  asked with its own.
- Docker Hub sources announce under new identities once; herald has no
  release whose cursors would re-announce them.
