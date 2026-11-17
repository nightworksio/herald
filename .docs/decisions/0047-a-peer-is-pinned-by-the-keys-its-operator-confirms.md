# ADR-0047: A peer is pinned by the keys its operator confirms

**Status:** Accepted
**Date:** 2026-11-17

## Context

Instances find each other by their discovery documents, sign every answer
and prove every envelope with Ed25519 (ADR-0043, ADR-0044, ADR-0046). A
herald that reads another's feed, or later takes its pushes, must know
which keys stand for it. A key read from the network the first time it is
seen can be anyone's, so trusting it would let whoever sits between the two
instances that day speak for the peer from then on.

## Decision

1. **`[peers.<name>]` says who a peer is, how it is reached and what bounds
   it, and nothing it may do**: what it may do sits beside each resource.
   Its name is lowercase letters, digits and inner `-`, as it names the
   peer in its events' identities. `url` is `https://`, a domain and any
   path its endpoints are under, with no port, user, query or fragment.
   `keys` pins at least one Ed25519 Multikey, each once. `rotation` is
   `manual` (only the operator pins a new key) or `signed` (a new set the
   peer proves with a pinned key replaces them); `rate` bounds its requests
   (`"120/m"`, at most `"480/m"`), `most` the bytes read of its bodies
   (`"2MiB"`, at most `"8MiB"`), `hops` the instances its objects may have
   passed through (2, from 1 to 8), and `private` says whether private
   events may go to it (false). Each default and bound is one constant.
2. **A key stands for one peer, and a revoked key for none.** A config that
   pins one key for two peers does not load. `[federation] revoked` lists
   keys never trusted again, and a config pinning one does not load.
   `[federation] fields` (`"8KiB"`, at most `"32KiB"`) bounds each signature
   field and `Content-Digest` a peer sends before it is parsed.
3. **A peer's answer is read only once a trusted key signed it** over its
   status, `content-type`, `Content-Digest` and the URL asked (`@target-uri`
   through `;req`), made and expiring within `[server]
   inbound.replay_window` of now and living at most the 20 minutes any
   herald may sign an answer for, its digest naming the body it came with,
   and its key found by `keyid` among those trusted. Its body is read up to
   the peer's `most` and no further. A peer is reached through `[http]`, as
   any host a config names is.
4. **`herald peer add <url>` never trusts on first use.** It reads the
   discovery document, refuses it unless its answer is signed by a key it
   lists, and shows each key's Multikey and fingerprint (`sha256:` and the
   SHA-256 of its 32 bytes in hex). It pins the keys only once the operator
   answers `y`, or pins exactly the keys `--key` names, each a Multikey or a
   fingerprint the peer's operator gave out of band, asking nothing; there
   is no way to pin whatever the document lists unseen. `--print` prints
   the table and writes nothing, `--update` pins the keys a peer already
   named lists now, and a revoked key is never pinned. The table is written
   into herald.toml in place, every other line kept.
5. **`herald peer test`** reads each peer's discovery document, checked
   against its pinned keys, and names each key it lists that is not pinned
   and each pinned key it no longer lists; it exits 1 where one fails or
   changed. **`herald peer list`** shows each peer's keys and fingerprints.
   **`herald peer revoke <name>`** takes the peer out of herald.toml and adds
   its keys to `[federation] revoked`, so a restored copy of its table
   cannot bring them back; `--key` revokes one key and keeps the peer while
   it pins another.

## Consequences

- An operator trusts a peer as far as the fingerprints it compared, and a
  stolen key stops counting once revoked, even where an old table returns.
- A `herald` source pulls a peer's feed through the same check, and
  `rotation`, `rate`, `hops` and `private` bound the pulls, pushes and
  relays that follow.
