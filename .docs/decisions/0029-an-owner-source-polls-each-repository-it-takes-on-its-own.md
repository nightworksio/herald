# ADR-0029: An owner source polls each repository it takes on its own

**Status:** Accepted
**Date:** 2026-11-14

## Context

A forge source announces one repository's releases (ADR-0003). An
organization that ships dozens of repositories, such as one plugin per
repository, needs one source per repository, and a new repository goes
unannounced until someone adds its source. Forges list an owner's
repositories with the attributes an organization sorts them by: topics,
visibility, archived, fork, template, language, and on GitHub custom
properties. The stores herald runs on bound a record's size: Deno KV takes
at most 64 KiB, a GitHub repository variable 48 KB and a DynamoDB item
400 KB.

## Decision

1. **A forge source names an `owner` in place of a `repository`**: a GitHub
   or Forgejo organization or user, or a GitLab group's full path or a user.
   Any source kind can expand to many through the `Expand` trait; a source
   is polled as one (`Polling::One`) or as its members (`Polling::Many`).
2. **Members are chosen by a selection.** `include` and `exclude` are
   case-insensitive name globs with `*`, `?` and `[...]`, and `exclude`
   wins. `topics` and `language` are any-of; `properties` needs every named
   property to hold one of its values. Archived repositories, forks and
   templates are left out, and only public repositories are taken, unless
   the source says otherwise. A GitLab group's subgroups are members unless
   `subgroups = false`. A member's project name is its repository's name.
3. **Each round lists the owner at the source's `api`**, page by page, each
   page conditional on the `ETag` the last listing kept. Where every page
   is unchanged and the page after the last is empty, the listing is
   unchanged; any changed page reads the listing again from its first page.
   The listing keeps a digest of what it asked for (the owner, the `api`
   and the selection); a listing kept for anything else is read whole.
   Every kept member is checked against the current selection each round,
   whatever the listing found, and one it no longer takes leaves.
   An owner with no organization of its name is listed as a user, and the
   listing remembers which.
4. **Two caps refuse the source for the round, naming the setting:**
   `most_listed` (10000) bounds what the forge lists before the filters,
   and `most_members` (1000, at most 100000) what the selection keeps.
5. **A listing that fails keeps the last membership**: the members it holds
   are polled, none is added and none dropped, and the failure is reported.
6. **A member found after the first listing announces only what it
   publishes after it was found**; the members of the first listing
   announce their newest `backfill`. A member that leaves the selection is
   not polled from then on, and its cursor is kept for `keep_members`
   ("30d"); the owner's poll forgets it after that. A member that comes
   back within it resumes its cursor.
7. **Each member is one record, its cursor beside its membership**, at
   `cursors/<source>/<member id>.json`, keyed by the forge's stable id, and
   the owner's listing is one small record at `listings/<source>.json`,
   holding the owner's kind, each page's `ETag` and when it was listed. Both
   are written with compare-and-set through the `Cursors` port, so every
   store that keeps cursors keeps them, at any number of members. The
   listing is written only once every member it changed is, so a listing
   found unchanged never hides a member.
8. **A route declares its place private with `private = true`**; a place is
   public otherwise. Under `[doctor] private_to_public = "refuse"`, the
   default, a config whose route takes a source's private or internal
   repositories to a public place does not load, and an event from a member
   not everyone may see is announced only to `private` routes. `"warn"`
   loads it and doctor names the route; `"allow"` says nothing. A
   repository whose visibility the forge does not name, or names in a way
   herald does not know, is private. The field and the setting belong to the route and
   to doctor, not to any one kind of source.
9. **`herald doctor --online` lists each owner**, notes how many
   repositories it takes, and names as a mistake a listing over a cap and
   any two members, of one owner or of two, that share a project name.

```toml
[sources.plugins]
kind = "github"
owner = "lemonfiber"
include = ["plugin-*"]
visibility = ["public", "internal"]
properties = { team = "media" }

[[routes]]
id = "plugins"
sources = ["plugins"]
private = true

[doctor]
private_to_public = "refuse"
```

## Consequences

**A new repository is announced without touching the config**, from the
round after it is created.

**An unchanged owner costs one request per page and one more**, each page
answered `304 Not Modified` where the forge offers it.

**Membership fits every store**: one record per member keeps each record
small, whatever `most_members` is.

**A private repository never reaches a public place by accident**: the
config must say the place is private, or turn the refusal off.
