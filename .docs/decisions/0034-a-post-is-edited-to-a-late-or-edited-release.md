# ADR-0034: A post is edited to a late or edited release

**Status:** Accepted
**Date:** 2026-11-15

## Context

A tag and its Release share one identity, so each route announces a
version once (ADR-0031). A Release that comes after its tag's post (a tag
announced at once, or one whose window passed, ADR-0032) found that
post's entry and sent nothing, leaving the tag's text in place. A forge
Release whose name or notes are edited after its post was also left as
first announced.

## Decision

1. **The ledger records what a post shows.** Each entry keeps a revision:
   the signal of the event announced and the SHA-256 of every part the
   route rendered. An entry written before this keeps none, and is never
   edited.
2. **A late Release edits the tag's post.** Where the post shows a tag and
   a Release of its version now renders otherwise, the route's
   `late_release` says what happens: `edit` (the default) edits the post
   to the Release, and posts the Release anew where the destination
   cannot edit; `post` posts it anew, keyed by its identity and
   `#release`; `skip` leaves the tag's post.
3. **An edited Release edits its post where the route asks.** A route
   with `edits = "release"` edits a Release's post when its name or notes
   change; `edits = "none"` is the default. An edit of a release no route
   announced posts nothing.
4. **An edit is a set of steps.** The post and each chunk that landed are
   edited in place (`Edit`), each part more is posted as a chunk, each
   chunk fewer is deleted (`Delete`), the thread opened from the post is
   renamed (`Rename`), and the Note's `Update` is delivered to each
   follower its `Create` reached (`Updates`), each delivery kept under a
   key of the revision. An edit lands once per revision; an edit,
   deletion or rename that may have landed is sent again rather than read
   back. Discord (a bot or a webhook), Slack's bot, Matrix (an `m.replace`
   event and a redaction), Mastodon and ActivityPub edit; a Slack incoming
   webhook does not. An ActivityPub Note carries every part, so its
   `Update` carries a chunk's removal and no `Delete` is sent.
5. **A forge says a Release was edited, or a poll finds it.** GitHub's
   `edited`, Forgejo's `updated` and GitLab's `update` release events are
   announced as edits. A poll keeps a digest of the name and notes of the
   newest `edits_depth` Releases (30 where absent, at most `page`) and
   announces each whose digest changed as an edit. It watches only where
   a route taking the source has `edits = "release"`; otherwise it keeps
   no digests and asks nothing more. An owner polled in GitHub GraphQL
   batches then asks each Release in full in its first request.
6. **doctor says what cannot be edited.** A route that cannot edit is told
   that a late Release is posted anew and an edited one left; the markers
   store, which records nothing of what a post shows, edits no post and
   posts no late Release anew.

## Consequences

- A tag announced at once becomes its Release's announcement in place.
- An edit costs one request per part on most destinations, and one
  delivery per follower on ActivityPub.
- A cursor watching for edits grows by one digest per watched Release.
