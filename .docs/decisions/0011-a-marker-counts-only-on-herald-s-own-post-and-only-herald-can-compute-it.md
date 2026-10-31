# ADR-0011: A marker counts only on herald's own post, and only herald can compute it

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

Two defences close it, and each holds if the other fails.

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

   The chunks herald reads back from a thread are counted by the same rule.
   A Discord webhook cannot list a channel's messages, so a webhook
   destination reads no markers at all (ADR-0004).

2. **A post carries a marker, not the key.** The marker is the
   HMAC-SHA256 of `herald/marker/v1\0` followed by the key's 32 bytes, under
   the installation's marker key, in lowercase hex. It is computed through
   the `Signer` port (ADR-0001), so the core never holds the marker key.
   Without the marker key, nobody can compute a marker for a key, even if
   the author check is ever bypassed. Each place ADR-0004 named for the key
   carries the marker instead:

   | Destination | The marker is carried in |
   |-------------|--------------------------|
   | Discord | the fragment of the embed title's link (`…/releases/tag/v1.2.0#herald-<marker>`) |
   | Slack | the message's metadata |
   | Matrix | a namespaced field of the event's content, `io.nightworks.herald.marker` |
   | Mastodon | the fragment of the post's link |

   The ledger still records entries by key. Only what a destination shows
   changes.

3. **The marker key is `HERALD_MARKER_SECRET` where it is set, and is
   otherwise derived from the destination's credential.**
   - **The installation secret.** `HERALD_MARKER_SECRET` is a secret like any
     other (ADR-0007): named in `herald.toml`, never written in it.
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

4. **This supersedes in part:**
   - ADR-0004, decision 8: the place each platform keeps carries the marker,
     not the key;
   - ADR-0005, decision 7: the store finds the marker on herald's own posts,
     not the key on any post.

   Every other part of both decisions stands.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **The author check alone** | It rests on one field the platform reports. A bridge, a relay or a future platform change that misreports authors would bring suppression back. A marker nobody else can compute holds even then. |
| **The marker alone** | Anyone who can post where herald posts can copy a real marker from an earlier post into a planted one. A copied marker matches only the announcement it came from, but reading only herald's own posts removes even that. |
| **A required marker secret, with no fallback** | A newcomer would need a second secret before the first announcement. The fallback is as unforgeable, and doctor says what it costs. |
| **The credential alone, with no installation secret** | Rotating a leaked token would make herald forget which posts are its own. A separate secret keeps a token rotation from becoming a repost. |

## Consequences

**A planted or guessed marker sends the announcement anyway.** A message by
anyone else is never read as herald's, and a key computed from the config
matches no marker.

**Reading markers costs one more request on Discord.** The store asks which
user the bot is before it reads the channel.

**A rotated credential under the fallback forgets earlier markers.** Doctor
says so wherever the fallback is in use, and `HERALD_MARKER_SECRET` removes
the cost.

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): the `Signer` and `Markers` ports
- [ADR-0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md): what doctor names
- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): where each platform carries the marker; decision 8 is superseded in part
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the `markers` store; decision 7 is superseded in part
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): secrets, and what leaves herald
