# ADR-0035: A source announces the versions of its repositories' packages

**Status:** Accepted
**Date:** 2026-11-16

## Context

A forge source announced a repository's Releases and tags (ADR-0031). The
same repositories publish packages to their forge's registry: container
images to ghcr.io or a GitLab or Forgejo registry, and npm, Maven,
RubyGems, NuGet, PyPI, Cargo and other packages. A registry source
(ADR-0003) follows one package, so a project with several packages needed
a source for each, and a forge source could not announce a package beside
the repository's Releases. A container image a forge source announces
must not be announced again by a `ghcr` or `oci` source of the same
image.

## Decision

1. **`signals` takes `"package"`.** A forge source, or a member override,
   whose signals take it announces each new version of the packages the
   forge's registry holds for the repository as a `package-version` event
   whose `signal` attribute is `"package"`. The kind's optional `package`
   and `package_type` attributes name the package and its type as the
   forge writes it. The title is the package's name and version, the
   project the repository's (or the member's, as its template renders
   it), and the link the version's page.
2. **Every type, narrowed by settings.** `package_types` names the types
   taken, every one where absent; on GitHub, whose API asks for one type
   at a time, they are npm, maven, rubygems, docker, nuget and container,
   and any other name does not load. `packages` takes names by glob,
   every name where absent. These keys, `moving`, `registry` and
   `most_packages` do not load on a source whose signals never take
   packages. GitHub's packages API answers only with a token, so a GitHub
   source taking packages without one does not load.
3. **A container version is one event per tag**, under the registry's
   identity `<registry>/<owner>/<image>/<tag>`, lowercased but for the
   tag, which a `ghcr` or `oci` source of the same image shares, so a
   route announces it once (ADR-0005). The tags `moving` names by glob
   (`["latest"]` where absent) are never announced; an untagged version,
   and a Forgejo container version named by its digest alone, announce
   nothing. `registry` names the registry's host; where absent, it is
   ghcr.io beside api.github.com, `containers.<host>` beside a GitHub
   Enterprise Server's or a GHE.com tenant's API, the host in each image's
   location on GitLab, and the instance's on Forgejo. Every other type
   takes `<forge host>/<type>/<owner>/<package>/<version>`; GitHub's
   legacy `docker` type is one of them.
4. **GitHub and Forgejo list an owner's packages, GitLab a project's.**
   GitHub lists `/orgs/<owner>/packages` (or `/users/<owner>/packages`)
   for each type and asks each package's versions apart; Forgejo lists
   `/packages/<owner>`, each entry a version. Each keeps the packages
   linked to the repository. GitLab lists `/projects/<id>/packages`
   newest first and the project's container images and their tags, and
   reads an unseen tag in full for its time.
5. **An owner source lists its packages once a round** (`Expand::rounded`),
   after the membership listing and before any member is polled, and
   gives each member its own packages; a member with none sends no
   package request. A member the listing names is polled that round
   whatever the owner's events say (ADR-0030). A listing that fails is
   reported, and each member's poll that round leaves its packages as
   its cursor had them. GitLab owners poll each member's project.
6. **A round keeps the newest `page` versions** across all of a
   repository's packages as events; every other version it finds is
   remembered as seen, unannounced, so a version that was never among the
   newest is not announced when another package goes quiet. A listing
   reads at most `most_packages` entries (1000, at most 100000) before it
   refuses the round, naming the setting.
7. **Each package's versions are asked with their own validators**,
   which the cursor keeps by package beside the signals' (`packages`);
   GitLab's project listing is one entry of them. A poll whose every
   answer is unchanged announces nothing.
8. **A new source announces its newest `backfill`** package versions, as
   it does Releases. A source that polled before it took packages, or a
   member that joins after the owner's first listing, remembers the
   versions it first finds without announcing them. GitLab's image tags
   carry no time in their listing, so a new source remembers the tags it
   first finds without announcing any.
9. **A webhook of a package polls its source.** GitHub's
   `registry_package` and `package` deliveries (published or updated) and
   Forgejo's `package` delivery (created) poll the source, as a tag's
   delivery does (ADR-0032); an owner source first checks the delivery's
   repository against its selection and membership. A delivery of a
   package linked to no repository announces nothing. GitLab sends no
   package webhook, so its sources poll.
10. **The prerelease rule applies** to package versions. No forge flags
    one, so `prerelease = "flag"` marks none, and the version decides
    under `"either"` and `"tag"`.
11. **A package is seen as the stricter of its own visibility and its
    repository's**, by default. Its own is GitHub's package `visibility`
    (public, private or internal), the project's on GitLab, whose packages
    follow it, and the owner's on Forgejo (`limited` is internal); the
    repository's is the member's as its owner's listing names it, or the
    repository's as the forge describes it. A visibility the forge does
    not name, or one herald does not know, is private. A public package of
    a private repository is private, as its announcement would tie a
    public place to a private repository's activity. `package_visibility
    = "package"` lets the package's own visibility decide. The event
    carries the result, and a version not everyone may see reaches only
    `private` routes where `[doctor] private_to_public` refuses
    (ADR-0029), on a repository source as on an owner's member.

## Consequences

- A repository's Releases, tags and packages come from one source, and a
  route `kinds = ["package-version"]` or a template reading
  `event.attributes.signal` tells them apart.
- A GitHub source taking every type sends six listing requests a round
  for its owner, and an owner source sends them once for all its members.
- A Forgejo owner source reads its owner's whole package listing each
  round, within `most_packages`; a single repository's poll stops once it
  has a page of the repository's versions.
- A test against a `ghcr` source sharing a container identity waits for
  that source kind.
