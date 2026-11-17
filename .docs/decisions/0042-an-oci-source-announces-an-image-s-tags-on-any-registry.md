# ADR-0042: An oci source announces an image's tags on any registry

**Status:** Accepted
**Date:** 2026-11-17

## Context

Docker Hub (ADR-0003) and ghcr (ADR-0040) announce images through their own
APIs, which list tags with times. Every other registry, including GitLab's,
Quay, Harbor, a cloud provider's or a self-hosted `registry:2`, serves only
the OCI Distribution API. Its `tags/list` names tags in lexical order with no
time. Its token service may live on another origin, and its blobs are often
redirected to a CDN. The image's identity `<registry host>/<owner>/<image>/<tag>`
is the one the forge's `package` signal, ghcr and Docker Hub give
(ADR-0035), so a route announcing several of them announces each tag once.

## Decision

1. **`kind = "oci"` takes one `repository` or, with `catalog = true`, every
   repository the registry's `/v2/_catalog` lists,** each polled on its own
   as an owner's members are (ADR-0029), with `include` and `exclude` globs on
   the full path, `most_listed`, `most_members` and `keep_members`. `registry`
   is the registry's base URL.
2. **Tags are listed page by page with `n` and `last`.** `listed_page` sets
   the page size (1000, at most 10000). A listing over `most_tags` (10000, at
   most 100000), or one that names a tag twice, is refused for the round.
   `moving` tags (`["latest"]`) are never read or announced.
3. **The cursor remembers every listed tag,** as the listing holds no time
   to tell a new tag from an old one. `remembered` defaults to `most_tags`
   and may not be less. A store with no cursor (`markers`) refuses the
   source. A store whose records are smaller than such a cursor can grow
   (`most_record`, at a 128-character tag each) refuses a `most_tags` or
   `remembered` the source names. Where the source names neither, the store
   lowers `most_tags` to the most its records hold (git-branch about 6000,
   deno-kv about 400), and the doctor names the value.
4. **The first poll records every tag as seen without reading it.** With
   `backfill`, it reads the last `page` tags the listing names and announces
   the newest. A cursor that has seen nothing reads `page` tags and records
   the rest. Later polls read at most `page` unseen tags, the last listed
   first, and leave the others for the next poll.
5. **A tag's time is when its image was created:** the
   `org.opencontainers.image.created` annotation on its manifest or its
   index's first platform entry (attestations skipped), else the platform
   manifest's config blob `created`. A time that is missing, at the epoch,
   or behind a blob redirect herald may not follow becomes the `Date` the
   registry answered the manifest (or the listing) with, the time the tag
   was first seen, and without one the round's own time. Whoever pushes an
   image writes its `created` and the registry its `Date`, so a time more
   than five minutes past the round's is not believed either, and no tag
   moves the mark of decision 6 past the present. `Poll::polled` and
   `Expand::polled` take the round's time for this.
6. **A tag created before the newest tag announced is recorded, not
   announced.** The cursor keeps that mark (`Cursor.newest`), and it
   advances only on a poll that read every unseen tag, so new tags read
   across several polls are compared with the mark they arrived under.
7. **A `Bearer` challenge is answered only from the registry's own origin
   or the one `auth` origin.** `auth` defaults to `https://auth.docker.io`
   for Docker Hub's registry hosts. A realm anywhere else is refused, and
   `doctor --online` names it at `auth`. `username` and `token` go to the
   realm as HTTP Basic, or to the registry where it challenges for Basic.
8. **A config blob redirect is followed once, without the registry's
   credential, only to an origin `blobs` names.** The defaults are Docker
   Hub's and GitHub's CDNs for their registries. `doctor --online` reads one
   tag and names any other origin a blob is redirected to. The source's
   `Http` reaches the registry, `auth` and `blobs` (ADR-0007).
9. **Identity and origin.** A tag's identity is
   `<registry_name>/<repository in lowercase>/<tag>`. `registry_name`
   defaults to the registry's host, or `docker.io` for Docker Hub's registry
   hosts, where a one-name repository is `library/<name>`; a mirror may name
   the registry it mirrors. The event's origin is forge `oci`, that host, the
   repository's owner and path. The project is the repository, without
   Docker Hub's `library/`. Repositories are public without a `token` and
   private with one, unless `visibility = "public"` (ADR-0039).

## Consequences

- Any OCI registry is announced without an API of its own, and a route
  taking it beside Docker Hub, ghcr or a repository's `package` signal
  announces each tag once.
- A poll costs the listing's pages plus up to four requests for each tag it
  reads (manifest, platform manifest, config blob, its redirect), at most
  `page` tags a poll.
- An oci cursor holds every tag, so it is the largest record a source
  writes.
