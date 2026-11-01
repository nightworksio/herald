# ADR-0011: Markers count only on herald's own posts, a route keeps one pin, and a lock archives

**Status:** Accepted
**Date:** 2026-11-01

## Context

The `markers` store reads a destination's recent posts for the announcement
they mark (ADR-0005, decision 7). ADR-0004 (decision 8) puts the
announcement's never-twice key in each post, for example in the fragment of a
Discord embed's link.

The key is a SHA-256 of the route id and the event's identity (ADR-0005).
Both are public: the route id is in a committed `herald.toml`, and the
identity is a release's tag. So anyone who can post where herald posts can
compute a key and plant it in a message or an embed of their own, before the
release goes out. herald would then read the announcement as already posted
and send nothing. That is suppression: an announcement silently dropped by
someone the config never trusted.

Discord's housekeeping (ADR-0004, decision 4) pins the newest release and
unpins the one it replaces. One channel can carry several routes, as
`package-releases` carries php-mutation-gate's releases and herald's, so the
pin to replace is the same route's, not any earlier pin. A forum keeps a
single pinned post, set by a flag on the post's thread rather than by a
message pin. And Discord changes no archived thread unless the same request
unarchives it, so a thread archived after a week cannot simply be locked
after a month.

## Decision

1. **A marker counts only on a post by herald's own identity.** The markers
   store reads only posts written by the identity herald posts as, and
   ignores a marker on anything else:

   | Destination | herald's identity |
   |-------------|-------------------|
   | Discord | the bot's user id, read from `GET /users/@me` |
   | Slack | the bot's `bot_id` and `app_id` |
   | Matrix | the sender's Matrix user id |
   | Mastodon | the account id |

   The chunks herald reads back from a thread, and the pins it replaces
   (decision 5), are counted by the same rule. A Discord webhook cannot list
   a channel's messages, so a webhook destination reads no markers at all
   (ADR-0004).

2. **A post carries two markers, not the key.**
   - **The announcement's marker.** The HMAC-SHA256 of `herald/marker/v1\0`
     followed by the key's 32 bytes.
   - **The route's marker.** The HMAC-SHA256 of `herald/route/v1\0`
     followed by the route id, the same on every post of the route.

   Both are computed under the installation's marker key (decision 3),
   through the `Signer` port (ADR-0001), and written in lowercase hex, so
   the core never holds the marker key. Without it, nobody can compute a
   marker for a key or a route, even if the author check is ever bypassed.
   Each place ADR-0004 named for the key carries the markers instead:

   | Destination | The markers are carried in |
   |-------------|----------------------------|
   | Discord | the fragment of the embed title's link (`…/releases/tag/v1.2.0#herald-<route>.<announcement>`) |
   | Slack | the message's metadata |
   | Matrix | namespaced fields of the event's content, `io.nightworks.herald.route` and `io.nightworks.herald.marker` |
   | Mastodon | the fragment of the post's link, as on Discord |

   The ledger still records entries by key. Only what a destination shows
   changes.

3. **The marker key is `HERALD_MARKER_SECRET` where it is set, and is
   otherwise derived from the destination's credential.**
   - **The installation secret.** `HERALD_MARKER_SECRET` is a secret like any
     other (ADR-0007): named in `herald.toml` as
     `[markers] key = { secret = "HERALD_MARKER_SECRET" }`, never written in
     it.
     `herald init` prints a new 256-bit random one once, for the adopter to
     store as that secret.
   - **The fallback.** Without it, the marker key is the HMAC-SHA256 of
     `herald/marker-key/v1` under the destination's own credential: the bot
     token, or the webhook URL. A newcomer needs no second secret to start.
   - **What doctor says.** Doctor names a missing `HERALD_MARKER_SECRET`
     (ADR-0002). Where the fallback is in use, it says so plainly, and says
     what it costs: rotating that token forgets every earlier marker, so a
     rerun of an announcement made before the rotation posts it again on a
     store that has only markers.

4. **A route that pins keeps one pin, found as its config says.** A route's
   `pin` is `"markers"` or `"store"`, and pinning is off where it is absent.
   - **`markers`, the default.** The unpin step reads the destination's pins
     and unpins each of herald's own that carries the route's marker, other
     than the announcement's. It needs no store, so every store serves it.
   - **`store`.** The store keeps each route's pinned post, and the unpin
     step unpins the one it remembers. The `Store` port holds a fifth kind
     of record, a pin per route, with the last written kept. A store
     declares it as a capability, and the conformance suite holds every store
     that declares it (ADR-0005, decision 8). Doctor refuses `"store"` on a
     store that keeps no pins, such as `markers`, and says to use
     `"markers"` or a store that keeps them.
   - **An earlier pin already gone** counts as unpinned.

5. **Each destination says how many pins it holds, and the steps follow.**
   - **Many pins**, as a Discord channel: the announcement is pinned, then
     the route's earlier pin is unpinned, so the channel is never without
     one.
   - **One pin**, as a Discord forum: the earlier post is unpinned first, as
     the forum takes no second.
   - **A Discord forum pins by the post's `PINNED` flag.** Under `markers`,
     the unpin step finds the forum's guild, lists its active threads, and
     takes each thread in the forum that the bot owns, is flagged `PINNED`
     and whose opening message carries the route's marker. Under `store`, it
     takes the post the store remembers.

6. **A lock archives.** Locking a Discord thread sends
   `{"archived": false, "locked": true}`, then `{"archived": true}`. A
   locked thread is therefore archived too, and only a moderator reopens it,
   whether or not it was archived before.

7. **This supersedes in part:**
   - ADR-0004, decision 8: the place each platform keeps carries the
     markers, not the key;
   - ADR-0005, decision 5: the `Store` port holds a fifth kind of record,
     each route's pin;
   - ADR-0005, decision 7: the store finds the marker on herald's own posts,
     not the key on any post.

   Every other part of these decisions stands.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **The author check alone** | It rests on one field the platform reports. A bridge, a relay or a future platform change that misreports authors would bring suppression back. A marker nobody else can compute holds even then. |
| **The marker alone** | Anyone who can post where herald posts can copy a real marker from an earlier post into a planted one. A copied marker matches only the announcement it came from, but reading only herald's own posts removes even that. |
| **A required marker secret, with no fallback** | A newcomer would need a second secret before the first announcement. The fallback is as unforgeable, and doctor says what it costs. |
| **The credential alone, with no installation secret** | Rotating a leaked token would make herald forget which posts are its own. A separate secret keeps a token rotation from becoming a repost. |
| **Only the route marker, or only the store, for pins** | The route marker serves every store but costs a read of the pins on each release. The store costs no read but needs a store that keeps records. Each is the better choice somewhere, so a route chooses. |
| **Unpinning every earlier pin of herald's** | Two routes in one channel would unpin each other, and only the last to post would stay pinned. |
| **No pins in a forum** | A forum route would lose what a channel route has. The flag and the unpin-first order give it the same. |
| **Locking without archiving** | Discord refuses to lock an archived thread without unarchiving it, so the lock would either fail or leave the thread open. |

## Consequences

**A planted or guessed marker sends the announcement anyway.** A message by
anyone else is never read as herald's, and a key computed from the config
matches no marker.

**Reading markers costs one more request on Discord.** The store asks which
user the bot is before it reads the channel, and so does the unpin step
under `markers`.

**A rotated credential under the fallback forgets earlier markers.** Doctor
says so wherever the fallback is in use, and `HERALD_MARKER_SECRET` removes
the cost.

**Routes sharing a channel keep a pin each.** A forum, which holds one, is
pinned by the route that posted last.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): the `Signer`, `Markers` and `Store` ports
- [ADR-0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md): what doctor names
- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): Discord's housekeeping, and where each platform carries the markers; decision 8 is superseded in part
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the `Store` port and the `markers` store; decisions 5 and 7 are superseded in part
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): secrets, and what leaves herald
