# ADR-0049: A herald source pulls a peer's feed through the keys that stand for it

**Status:** Accepted
**Date:** 2026-11-18

## Context

A herald publishes its announcements as proved envelopes in a signed,
paged feed (ADR-0046), and another pins the keys that stand for it
(ADR-0047). What reads that feed must take nothing the peer did not sign,
must not let one peer speak for a forge or another peer, and must follow a
peer that rotates its keys without trusting whoever answers on the day.

## Decision

1. **A `herald` source pulls a feed** from a peer of `[peers]` (`peer` and
   `feed`), or from an instance whose keys the source pins itself (`url`,
   the feed's full URL, and `keys`), within the bounds a peer takes where
   it says nothing. A key it pins that a peer pins too, or that
   `[federation] revoked` lists, does not load; nor does a peer `[peers]`
   does not have. Its events may be of any kind the config declares.
2. **The first pull is quiet**, as every source's (ADR-0003): it remembers
   the feed's newest sequence and announces nothing but the newest
   `backfill`, at most `pages` pages of 80. Later pulls ask the first page
   with `If-None-Match` and walk `?before=` down to the sequence read, at
   most `pages` (5, at most 20) a pull. The cursor keeps the sequence.
3. **Every answer is checked before its body is read**, as a discovery
   document is (ADR-0047): signed by a key that stands for the peer over
   `@status`, `content-type`, `Content-Digest` and `@target-uri;req`,
   within `[server] inbound.replay_window` and the 20 minutes any herald
   signs for, its digest naming its body, read up to the peer's `most`. A
   page lists at most 80 envelopes, newest first, each at most 64 KiB, and
   is the page asked for. An answer that nothing changed (`304`) is taken
   only once signed the same way over `@status`, its `ETag` and
   `@target-uri;req`, its `ETag` the one asked for; an unsigned one stops
   the pull. The cursor keeps an `ETag` only where the answer's signature
   covers it, as herald's always does.
4. **An envelope is taken only once its proof holds for the instance that
   made it.** `verify = "origin"` takes envelopes whose path is the peer
   alone. `verify = "hop"` also takes one the peer passed on from another
   peer of `[peers]`, proved by that peer's key, its path at most the
   peer's `hops`, ending at the peer, and naming no instance twice. Any
   other path is passed over; a proof that does not hold stops the pull.
5. **No peer speaks for a forge.** Each identity is taken as
   `herald/<maker>/<identity>`, the maker being the peer or the peer that
   made a relayed envelope, named as `[peers]` names it, or an instance
   pulled by `url` by its domain, which no peer's name can be; its origin
   names the maker as a `herald` forge on its domain; the forge, host and
   repository the envelope claims are the attributes `claimed_forge`,
   `claimed_host` and `claimed_repository`, so a route matching this
   herald's own repositories never takes a peer's claim. `namespaces`
   grants identities the peer may give as they are, each the start of
   identities up to a `/`, never under `herald/`; a granted identity keeps
   the host and repository its origin names only where they lie under the
   same namespace, its forge still `herald` (an envelope proves neither its
   forge nor its origin's id), and only one the peer made itself is
   granted. An event of a kind the config does not declare, or not of its
   kind's shape, is passed over. An edited Release that enters the feed
   again is an edit, as a forge's is (ADR-0034), the digests of what the
   source saw kept for every identity it still remembers.
6. **No envelope turns an event back.** A proof made more than
   `[server] inbound.replay_window` ahead of the round is no proof: its
   envelope is passed over, nothing of it kept, its maker named in the
   poll's outcome and by doctor, and the pull reads on. The cursor keeps
   when the proof of the newest envelope it took of each identity was
   made, and by which instance, in the order it took them: every identity
   it took an event of, neither passed over nor only seen, that a
   tombstone withdrew, never forgotten, so no older envelope of a
   withdrawn release is taken again; the tombstones of identities it never
   took, the ones taken last, at most `peer_tombstones` (250, at most
   `tombstones`) of any one instance and `tombstones` (1000, at most 4000)
   in all, however many tombstones of identities it took any instance
   has it keep; and of the rest the ones each
   instance made that it took last, as many as it remembers shared out
   evenly among the instances whose envelopes it takes, so neither an
   instance taking many identities nor any time a peer writes in a proof
   decides which proofs of another it forgets. A tombstone of an identity
   the source took is always taken: withdrawing is the safe direction.
   Each instance may have it keep at most `peer_share` (1000, at most
   4000) tombstones of releases it withdrew, which are never forgotten: a
   pull takes no new identity of an instance at its share, names it in
   its outcome, and keeps its sequence before that envelope, its `ETag`
   dropped, so a later pull reads it again once the share is raised;
   doctor names each instance at its share. An envelope, or a tombstone,
   whose proof is no newer is passed over, and so is every place a page
   lists an identity after its first, so neither a replayed envelope nor
   one a peer relays edits, withdraws or brings back an event taken under
   a newer proof.
7. **A tombstone withdraws only what its own source announced.** Once
   its proof holds, and only for an identity the source took an event
   of, its identity is withdrawn from every route taking the source as the route's
   `withdraw` says (ADR-0048). Each ledger entry names the source whose
   event it announced, and a tombstone withdraws only a post, or a place
   in a group, whose entry names its own source: one another source
   announced, or whose entry names none, stays. A route announcing each
   project as an actor of its own keeps its post, as a tombstone names no
   project. The identity is no
   longer seen, so its release entering again under a newer proof is
   announced anew. A withdrawal that stops keeps the cursor where it was,
   and the next round finishes it.
8. **A signed rotation is taken and kept.** Herald proves the key set its
   discovery document lists by its oldest key. Where a peer's `rotation` is
   `signed`, a set proved by a key that stands for it under a later
   sequence, naming no revoked key, stands for it in place of its pinned
   keys while those stay pinned and it names no revoked key, kept in the
   store through the `Published` port with when it was read. It is read
   again after the set's `refresh` (at most 7 days), and once more within a
   pull whose answer is signed by a key not trusted. A kept set naming a
   key revoked since stands for nothing from the next pull on, and is read
   again at once; no revoked key verifies an answer, an envelope or a set.
   Another peer's set is read only by its own pulls.
9. **doctor names what a signed rotation risks**: each peer whose rotation
   is signed, the markers store that keeps no peer's set, and with
   `--online` each set taken that herald.toml does not pin. `herald peer
   add --update` pins that set without asking, as keys that stand for the
   peer proved it.

## Consequences

- A herald re-announces another's releases without letting it suppress or
  impersonate the real ones, and follows its rotations as far as its
  operator allowed.
- Pushing events to a peer, the grants beside each resource, and an audit
  of what peers ask build on the same checks.
