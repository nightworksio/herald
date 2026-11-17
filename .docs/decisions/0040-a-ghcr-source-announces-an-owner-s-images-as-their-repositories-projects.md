# ADR-0040: A ghcr source announces an owner's images as their repositories' projects

**Status:** Accepted
**Date:** 2026-11-16

## Context

A forge source's `package` signal announces the container images linked to
its repositories (ADR-0035), so an image linked to no repository, or one an
owner holds without a forge source, is announced by no source. A registry
source of the image shares the identity `<registry host>/<owner>/<image>/<tag>`
with the forge's, so a route announcing both announces each tag once.

## Decision

1. **`kind = "ghcr"` takes a GitHub owner's container images.** The owner's
   `/orgs/<owner>/packages?package_type=container` (or `/users/<owner>/…` where
   GitHub knows no such organization) is listed once a round, and each image
   is polled on its own with its own `ETag`, as an owner's members are
   (ADR-0029). `token` is required, as GitHub lists packages only with one.
   `api` names a GitHub Enterprise Server or GHE.com tenant; `registry`
   defaults to the host beside it (ghcr.io, `containers.<host>`).
2. **Its settings are a forge owner's and the `package` signal's:**
   `include` and `exclude` globs on image names, `visibility` (`["public"]`),
   `moving` (`["latest"]`, never announced), `most_packages` (1000, at most
   100000, refusing the round over it), `keep_members` ("30d"), and the
   reading settings.
3. **An image linked to a repository announces as that repository's
   project:** `project`, a template over its `repository.*` as an owner
   source renders it, or the repository's name. An image linked to none
   announces as its own name. `herald_core::Listed` carries the repository
   an image is linked to (`linked`).
4. **An image is seen as the stricter of its own visibility and its
   repository's,** as the `package` signal sees one (ADR-0035);
   `package_visibility = "package"` lets the image's own decide. The
   selection, `unlisted()` and each event's visibility all read it.
5. **An image may share its project with its repository's own source.**
   A source or member names the repository it stands for (`Standing`); the
   claim rule (ADR-0030) withholds a member whose project another claims,
   except where one is an image and the other the repository it is linked
   to. Both announce under the project, and a route announces each tag once
   by its identity. Two forge sources, two images, or an image and an
   unrelated repository still collide, and doctor names them.

## Consequences

- One route `match` on a repository's project covers its Releases and its
  images.
- An owner of N images costs a listing and N requests a round.
