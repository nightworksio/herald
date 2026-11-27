# ADR-0090: A Forgejo repository keeps herald's records on a branch, as a GitHub one does

**Status:** Accepted
**Date:** 2026-11-27

## Context

The `git-branch` store
([ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md))
keeps herald's records one file per record on a branch of a GitHub
repository. A write names the blob it replaces, and GitHub's contents API
refuses it once the file changed: that refusal is the store's
compare-and-set, and every record herald writes with it is either written
over the one it read or not written at all.

A project on Forgejo or GitLab has no such store: it keeps its records in a
database, the Actions cache or the destination's own posts. A store kept in
the repository is only sound where the forge's API refuses a write that
would replace a record someone else changed since herald read it.

## Decision

1. **The `forgejo-branch` store keeps the records on a branch of a Forgejo
   repository.** One file per record on `herald-state` (`branch`), sealed
   with the store's `key` as every store kept where others can write is
   (ADR-0005). It works with any Forgejo instance: `api` is its base URL,
   such as `https://forge.example/api/v1`; where absent it is the run's own,
   as Forgejo Actions sets `GITHUB_API_URL`, when the store names no
   `repository`, and Codeberg's when it names one. `repository` is the
   run's own (`GITHUB_REPOSITORY`) where absent, and `token` the secret
   `GITHUB_TOKEN`, as Forgejo Actions sets it, where absent. The token is
   read only as the store opens, sent as Forgejo takes one
   (`Authorization: token`), and held by the run's scrubber; a refusal is
   shown by its status alone.
2. **Forgejo's contents API is the compare-and-set.** A write of a record
   that exists names the blob it read (`PUT /repos/{owner}/{repo}/contents/{path}`
   with `sha`); a new record is created (`POST` there) and a record is
   removed (`DELETE` with `sha`). Forgejo checks the named blob against the
   file at the head of a clone of the branch, commits on that head, and
   pushes the commit to the branch without force, so:
   - a blob that is no longer the file's is refused, `409`, and herald reads
     the record again (`400` for a removal);
   - a record created where one exists is refused, `422`;
   - a write made while the branch moved since the check is refused by the
     push itself, which git takes only as a fast-forward of the head the
     check read; Forgejo answers that refusal `500`, and herald shows it as
     the store's refusal: nothing is written, and the next run writes again.
   No write replaces a record changed since herald read it. A branch moved
   back by a forced push to an earlier head takes herald's next write on
   top of that head, as whoever can push to the branch can put back what
   herald wrote there before (ADR-0005).
3. **A branch is made once.** herald's first write makes `herald-state` in
   an empty repository. In a repository that holds other branches it is
   made once, with no history, so none of the repository's files or
   workflows ride on it, such as with `git switch --orphan herald-state`,
   an empty commit and a push; until then a write says so.
4. **Reads are bounded.** A record is read whole through the contents API,
   at most 1 MB, as git-branch's are, and one Forgejo answered only in part,
   as an instance answers a file past its `[api] DEFAULT_MAX_BLOB_SIZE`,
   reads as corrupt. A directory is listed by the Git Trees API from the
   branch's head down its path, a page of 1000 entries at a time, and at
   most 100000 entries, as many as GitHub's Git Trees API answers of a
   recursive tree; a directory past that, or one Forgejo lists only in
   part, is refused.
5. **GitLab's files and commits APIs give no compare-and-set of a file.**
   They check a `last_commit_id` before the commit is made, apart from it:
   the commit is laid on the branch's head as it stands when it is made, so
   a change landed between the check and the commit is replaced. Naming the
   head read as `start_sha` on the branch itself is refused unless the
   write forces the branch to it. No store keeps herald's records in a
   GitLab repository.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **A `forge` option on `git-branch`** | Each forge's store reaches its own API with its own defaults, as each forge's source is a kind of its own (`github`, `gitlab`, `forgejo`). |
| **Make the branch from the default branch** | The branch would carry the repository's files and its workflows, and every record written would run them. |
| **List a directory through the contents API** | It answers every entry at once, with no page and no bound. |
| **A GitLab store checking `last_commit_id`** | A write between GitLab's check and its commit would be replaced, which no other store allows. |

## Consequences

- A Forgejo project, on Codeberg or its own instance, keeps herald's
  records in its repository with no database, as a GitHub project does.
- Two runs writing the branch at once may see one refused with `500`; the
  refused one writes again on its next run.
- A GitLab project keeps its records in any other store herald ships.

## Related

- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the stores, sealed records and the git-branch store this extends
- [ADR-0025](0025-routes-and-sources-go-at-once-within-limits-and-a-session-keeps-where-it-stands.md): a store that writes one record at a time
- [ADR-0026](0026-a-store-s-write-attempts-lock-and-tombstone-are-settings.md): how many times a write is tried
- [ADR-0037](0037-gitlab-owners-poll-through-graphql-and-forgejo-owners-follow-their-feed.md): Forgejo and GitLab sources
