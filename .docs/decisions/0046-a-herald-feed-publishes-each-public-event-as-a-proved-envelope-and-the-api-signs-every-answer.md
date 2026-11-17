# ADR-0046: A herald feed publishes each public event as a proved envelope, and the API signs every answer

**Status:** Accepted
**Date:** 2026-11-17

## Context

Peers find an instance by its discovery document and pin its keys
(ADR-0043, ADR-0044). They need somewhere to read its announcements from,
in a form whose origin a relay or mirror cannot change, and a way to tell
that each answer they read came from the instance and answers what they
asked.

## Decision

1. **A `herald` destination is a feed herald serves rather than posts
   to**, in the crate `herald-dest-herald`. A route to it publishes each
   event as an envelope: it renders no template, and a template on it does not
   load. Its envelopes are proved by the newest `[federation]` key, which
   stands for its credential, so a feed without `[federation]`'s `domain`
   and `keys` does not load.
2. **An envelope holds what of the event is public, named one by one**:
   its identity, kind, project, title, body, links, the attributes its
   kind declares, when it happened, its origin's forge, host and
   repository, and `visibility` `public`; never what its source keeps to
   itself, such as its origin's id, topics, custom properties or the source
   it reached herald through. It names `type` `herald:Event`, a `path` of
   the instances it passed through (this one first) and `evidence` naming
   its release link, and carries an `eddsa-jcs-2022` proof (ADR-0043). It takes at most `envelope` bytes
   (`"16KiB"`, at most `"64KiB"`), proof counted; a longer event's body is
   cut at a character boundary and the envelope marked `"cut": true`, and
   an event too long even without its body stops the route.
3. **The store keeps one record per feed** through the `Published` port:
   its newest `keep` envelopes (1000, at most 4000), newest first, each
   under a sequence never used twice, written with compare-and-set within
   the store's write attempts. A store whose records hold fewer fits an
   unset `keep` to them; an explicit `keep` they cannot hold does not load.
   The markers store keeps no feed, so a feed beside it does not load.
4. **An event enters once for each revision a route shows**, as the
   ledger keeps it: an edited or late Release enters again first, under a
   new sequence, in place of its older envelope, as the route's
   `late_release` and `edits` say.
5. **Only an event the routing sees as public enters.** The routing
   works out who may see each event (its own visibility, its origin's,
   private for an owner's member that says nothing, public otherwise), and
   the feed takes it only where that is public. Where a published event is
   no longer public, a tombstone (`herald:Tombstone`, its identity, when it
   was withdrawn and its path, proved) takes the envelope's place first,
   and the route's post is forgotten so the event enters again once it is
   public. A route declared `private` to a feed does not load where
   `[doctor] private_to_public` refuses.
6. **`GET /herald/v1/feeds/<destination>`** lists `page` envelopes (20, at
   most 80) newest first, each with its sequence, the page's URL, the
   feed's URL and the next page (`?before=<sequence>`) where older
   envelopes follow. Each page carries a strong entity tag of the feed's
   newest sequence, the cursor and the page size, and a reader holding it
   is answered 304.
7. **Every answer under `/herald/v1`, and the discovery document, carries
   an RFC 9421 signature** by the newest key over `@status`,
   `content-type`, its `Content-Digest`, its `etag` where it has one, and
   the request's `@method` and `@target-uri` through `;req`, so no answer
   passes for another URL's. It expires after `[federation] signed_for`
   (`"5m"`, at most `"20m"`). Every host carries the answer's headers.
8. **The discovery document and NodeInfo list each feed** a route
   publishes to, and the discovery document names the `feeds` feature.
   doctor names a layer with no endpoint, which serves no feed.

## Consequences

- A peer pulls an instance's announcements with a cursor and a tag, and
  checks each envelope against the key it pins, however many hops passed
  it on.
- A withdrawn event leaves a tombstone a follower can act on; the route's
  `withdraw` setting and `herald withdraw` build on it.
