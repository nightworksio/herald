# ADR-0054: A peer pushes into a herald source it is granted

**Status:** Accepted
**Date:** 2026-11-18

## Context

A `herald` source pulls a peer's feed on herald's schedule (ADR-0049), so
what the peer announces waits for the next pull, and an instance that
cannot be reached by its peers, such as one on GitHub Actions, is never
pulled at all. A peer pushing to the source closes both, but only where
the operator granted it, for as long as they granted it, and with a
record of every request to read back.

## Decision

1. **A grant sits beside the resource and the peer, and both must agree.**
   A `herald` source's `peers = { <peer> = { actions, until, namespaces } }`
   and the peer's `grants = [{ resource = "sources.<source>", actions,
   until, namespaces }]` name the same grant, or herald.toml does not load,
   naming both places. The only action is `push`; an unknown one does not
   load. `until` is an RFC 3339 string or a TOML datetime with its offset,
   never where absent; `namespaces` grants identities as a pulled source's
   `namespaces` does. A source naming no feed takes only what the peers it
   grants push.
2. **The inbox reads a push only once its peer is known.** `POST
   /herald/v1/inbox/<source>`, on the instance's domain alone, takes one
   envelope or tombstone. Nothing of a request is kept before one of its
   sender's rates counts it. A body larger than any peer's `most` is
   refused before anything is read. The request must carry an RFC 9421
   signature over `@method`, `@target-uri`, `content-type` and
   `Content-Digest`, created and expiring within `[server]
   inbound.replay_window`, living at most 20 minutes, with a nonce, by a
   key that stands for a peer as kept, never read again by a push. The
   body is held to that peer's `most` before it is read; only then must
   its digest name it and the signature hold. Then the nonce is looked up,
   writing nothing. A nonce seen before counts only against the peer's
   replays (`[federation] replay_rate`, 120 a minute, at most 480), never
   its own rate, so no one replaying a request they captured spends the
   peer's. A new nonce counts against the peer's `rate`, however the push
   is answered after, and is admitted, so a signed request is taken at
   most once until it expires, even one refused past the rate and sent
   again once the window turns; a nonce admitted meanwhile by a push
   racing it is a replay. Each count is a window the store keeps; past one
   herald answers `429` and audits the first request of the window and no
   other. Of requests refused past its rate, a peer has at most
   `[federation] refused_nonces` nonces kept every 20 minutes, the longest
   a request lives (2400, its default rate over that time, at most 9600).
   Past them the peer is held: its window keeps one time, the latest at
   which any request it made since is still live, and until then nothing
   of it is taken and nothing more is kept, one audit entry marking the
   hold. A request it made unkept has expired before the hold ends, so it
   is never taken. A replay past the replay rate is only logged. Only then
   is the source asked whether it
   grants the peer `push` now. A push a host hands on to be worked after
   it was answered, as the Lambda does, is counted and audited no more,
   and takes again the very proof the source kept for it.
3. **A push is taken as a pull takes it.** The envelope is read as one of
   the pushing peer's own pages: under `herald/<peer>/` but where its
   grant's `namespaces` give it as it is, its proof holding for the
   instance that made it and newer than the source's, its origin the
   peer's, within the source's budgets, each the pull's. Its proof is kept
   in the source's cursor, with compare-and-set, before herald answers
   `202`; the cursor keeps what a poll keeps of its own, such as its
   entity tag and place in the feed. Its event is then announced, or its
   tombstone withdraws the identity, as a pulled one would.
4. **Every answer is fixed.** `202` taken, `401` unsigned, signed by no
   peer's key, not holding or replayed, `403` not granted, ended, or not
   taken, `413` too large, `429` past the rate, `503` the store cannot
   answer; why goes to the log and the audit only.
5. **Every signed request is audited, never its body.** Each is kept in
   its peer's record of the UTC day once its signature holds, or under
   `-` where it names a key no peer holds or its signature does not hold,
   so no one but the peer writes in the peer's name or spends its rate.
   Those under `-` are kept within `[federation] stranger_rate` (120 a
   minute, at most 480), counted before the audit is touched; the rest,
   and one carrying no readable signature, are only logged. A record holds the time, the peer or the key id it named, the action, the
   resource, the status and why, at most 1000 a day and the rest counted.
   `[federation] keep_audit` (90 days, at most 365) bounds how long a day
   is kept, as housekeeping forgets older days. `herald peer log` shows
   them newest first, `--since` a day, a time or a length of time ago,
   `--limit` 1000 where absent, and `--format json`. A store that keeps no
   cursor keeps no herald source, so the markers store refuses an inbox
   at load.
6. **doctor names every grant** with its peer, resource, actions and end,
   each that never ends, and each ended or ending within `[federation]
   grants_ending` (7 days, at most 90). A peer pinning only revoked keys
   does not load, so every grant has a key to verify by.
7. **What a cursor keeps fits the store.** An envelope's identity and its
   project are at most 1 KiB each. The cursor's identities, their proofs
   and projects, the tombstones of identities it took, `peer_share` of
   each instance it takes from, and `tombstones` of others are counted
   against the most a record of the store holds at load; a source naming
   none of `pages`, `tombstones` and `peer_share` has them lowered to fit,
   and doctor names it. Each identity's project is kept, so a route
   announcing each project as an actor of its own withdraws as that
   project's actor.

## Consequences

- A peer's announcements arrive as they are made, and an instance its
  peers cannot reach still takes them, each push held to the same checks
  as a pull.
- The `herald-push` destination, which sends herald's own announcements
  to a peer's inbox, builds on this inbox.
- A push lost after its `202` is not pushed again by a well-behaved peer,
  as its proof is kept; the routes' own ledgers retry what they took.
