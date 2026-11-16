# ADR-0031: A source announces releases, tags or both, and tells a pre-release by its own rule

**Status:** Accepted; a tag waiting for its Release extended, and decision 6 (a webhook announces releases only) superseded, by [ADR-0032](0032-a-tag-waits-for-its-release-in-a-held-record-for-its-window.md); a late Release editing its tag's post extended by [ADR-0034](0034-a-post-is-edited-to-a-late-or-edited-release.md); a package signal extended by [ADR-0035](0035-a-source-announces-the-versions-of-its-repositories-packages.md)
**Date:** 2026-11-15

## Context

A forge source announced each published Release, and nothing else
(ADR-0003). Some repositories ship by tag alone: a release train tags
every repository at one version and publishes a Release for one of them.
A tag with no Release went unannounced, and a tag that later gets a
Release must not be announced twice. A forge source told a pre-release by
the forge's flag or a pre-release part in the tag, fixed. lemonfiber
flagged releases v0.1.0 to v0.16.0 as pre-releases while their tags say
they are not, and other projects name stable versions with a hyphen, so
no one rule fits every repository.

## Decision

1. **`signals` chooses what a forge source announces** of each
   repository: `"release"` (published Releases), `"tag"` (version tags),
   or both; `["release"]` where absent. It is set per source, and per
   member of an owner source in `[sources.<owner>.repositories."<glob>"]`.
   An empty list does not load.
2. **A tag and its Release share one identity**, `host/<repository
   id>/<tag>`, so each route announces the version once, whichever comes
   first, and the ledger keeps every route from sending it again
   (ADR-0005). Both are `release` events; the event's `signal` attribute
   (`"release"` or `"tag"`) tells them apart for templates. A tag's title
   is its name, its body its annotated message (empty for a lightweight
   tag), its link the page its Release has once there is one (GitLab: the
   tag's page), and its time the annotated tag's date, or else its
   commit's.
3. **Each signal is asked with its own validators**: the releases with
   the cursor's `ETag` and `Last-Modified`, the tags with those the cursor
   keeps for the tag signal. A poll whose every answer is unchanged
   announces nothing. A tag no Release of the same answer covers and the
   cursor has not seen is read in full where its listing leaves its time
   or message out: on GitHub through its reference and its tag object or
   commit, and on Forgejo through an annotated tag's object; GitLab lists
   every tag in full. A source's first poll reads only its newest
   `backfill` tags in full and remembers the rest as seen, unread. A
   member that joins an owner's selection after its first listing reads
   every tag in full, so a tag made after it joined is announced.
4. **A GitHub owner polled through GraphQL** asks each member for the
   releases and tags its signals take, the tags ordered by their commit's
   date, in the same two requests (ADR-0030). An owner read through its
   events polls a member a new tag names where any member may announce
   tags.
5. **`prerelease` chooses how a pre-release is told**: `"either"` (the
   forge's flag, or a pre-release part in the version; the default),
   `"flag"` (the flag alone) or `"tag"` (the version alone), per source
   and per member. A tag carries no flag, so under `"flag"` a tag is never
   a pre-release. GitLab's releases carry no flag, so `prerelease =
   "flag"` does not load on a GitLab source. The rule applies to polls,
   webhooks and a CI run's own event alike.
6. **A webhook announces releases only.** A source that does not take
   the release signal for the repository announces nothing from a
   release's delivery or a CI run's release event, and doctor says that a
   source taking webhooks announces its tags when it is polled.
7. **A template is dry-rendered against a tag's sample** as well as a
   release's, where its kind names a signal (ADR-0006).

## Consequences

- One route announces a release train's tags and its Release without
  posting a version twice.
- Polling the tag signal costs one request for the tags each round, and
  on GitHub's REST API up to two more for each new tag; GitLab costs none
  beyond the listing, and GraphQL none beyond the batch.
- A cursor written before this decision reads as having no tag
  validators, so the first poll of a tag-taking source asks its tags
  unconditionally.
