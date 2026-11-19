# ADR-0060: a static host serves the feeds as a signed log

**Status:** Accepted
**Date:** 2026-11-19

## Context

A publisher that runs herald only in GitHub Actions has no server to
answer a peer's pull, so its feeds (ADR-0046) reached no other herald. A
static host such as GitHub Pages can serve files, but cannot sign each
answer as `herald server` does (RFC 9421), so a host, or anyone between it
and the reader, could withhold the newest pages, serve an old copy of a
page, or roll the whole log back, and a reader checking only each
envelope's proof would not notice.

## Decision

1. **The feed API as static files.** Under `[federation.pages] url`,
   whose host must be `[federation] domain` and whose path becomes the
   instance's path, herald writes `.well-known/herald`, the discovery
   document as `herald server` serves it, and each feed as pages of fixed
   buckets of sequences: bucket k holds the sequences from k times the
   feed's page size, the newest bucket at `herald/v1/feeds/<name>` and
   each older one at `herald/v1/pages/<name>/<k>`, each page naming the
   next. A new envelope changes only the newest bucket, and one dropped
   from the feed empties its bucket, whose file goes. A `.nojekyll` file
   lets GitHub Pages serve `.well-known`.
2. **One signed manifest.** The discovery document lists, for each feed,
   the sequence of its newest envelope and the URL and SHA-256 digest
   (RFC 9530) of each page, newest first, under the log's sequence and
   the time it was written. Beside it, `.well-known/herald.sig` holds a
   `herald:Signed` statement naming the document's URL, its digest, the
   sequence and the time, proved by the instance's newest key with the
   eddsa-jcs-2022 proof envelopes carry. No page carries a signature of
   its own. This replaces the earlier answer of a detached signature on
   every page: a page's own signature cannot catch an old copy of that
   page served whole with its old signature, while a signed manifest
   naming every page's digest catches it, and a withheld or replayed page
   too, with half the files and requests.
3. **The sequence is herald's, kept by compare-and-set.** Each writing
   takes the next sequence of a record every store keeps
   (`published/log.json`), claimed before anything is written, so no two
   writings share one; a markers store keeps none, and the config is
   refused. The record keeps the digest of each file and the commit
   written, so the next writing knows what changed.
4. **Written at the end of each run that changed it.** `herald poll`,
   `herald announce` and `herald withdraw` end by rendering the log and
   comparing it with the record. Where a feed or the key set changed, or
   the log was last written `refresh` ago ("1d" where absent, at most
   "7d"), so its readers see it is current, herald writes it.
5. **Two targets.** A branch, `herald-pages` where absent, takes one
   commit per writing through GitHub's Git Data API (a GitHub Enterprise
   Server's through `api`), holding only the files that changed over the
   head's tree, with the pages no longer listed removed; where the head
   is not the commit the record names, every file, over no tree. The
   branch is moved only where its head is still the commit's parent; a
   run that finds it moved stops, and the next writes the log again under
   a new sequence. `artifact` names a directory each writing fills with
   every file, for actions/upload-pages-artifact; naming it alone writes
   no branch. Release assets are no target: their names hold no `/`, and
   they are served under github.com, not the domain the keys are bound
   to, so no reader could read the log from them as it reads it from the
   static host.
6. **The reader fails closed.** A `herald` source with `static = true`
   reads the discovery document and its signature, each at most the
   peer's `most` bytes, and takes the log only where the statement names
   that document and its digest, its proof holds for a key trusted for
   the instance, its sequence is no lower than the one its cursor last
   read (`Cursor.log`), and it was written no further ahead than the
   skew allows and within `stale_after` ("3d" where absent, at most
   "30d"). Each page is read only where its bytes have the digest the
   document lists. A log it already read reads no page. A static source
   on a peer whose rotation is signed is refused, as the rotation is read
   from a signed answer a static host cannot give.
7. **doctor shows it.** The federation section (ADR-0058) names the
   log's URL, branch, directory and `refresh`, and each static source
   with its `stale_after`.

## Consequences

- A publisher on GitHub Actions alone serves its feeds to peers from
  GitHub Pages or any static host, and a reader catches a withheld,
  replayed or rolled-back page, and a log left unwritten past
  `stale_after`.
- Each writing makes one commit; a quiet feed still makes one commit per
  `refresh`.
- A key rotation reaches a static log's readers only by pinning, as with
  `rotation = "manual"`.
