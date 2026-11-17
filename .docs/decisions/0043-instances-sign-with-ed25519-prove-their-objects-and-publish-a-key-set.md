# ADR-0043: Instances sign with Ed25519, prove their objects, and publish a key set

**Status:** Accepted; the instance's keys, its discovery document and the key set's sequence extended by [ADR-0044](0044-an-instance-is-found-by-its-discovery-document-and-federation-holds-its-identity.md)
**Date:** 2026-11-17

## Context

herald instances are to talk to each other through a JSON API of
herald's own, for public announcements and for traffic between peers
alike; ActivityPub stays only as an outward mirror. Every request and
response between instances must say which instance sent it, and every
event object must say which instance it came from, however many relays
and mirrors pass it on. Each instance needs keys for both, published so
peers can pin them, and a way to replace them without asking every peer
to pin again by hand. ActivityPub's draft-cavage RSA signatures prove
neither the object nor its origin past one hop.

## Decision

1. **A new kit, `herald-peers`, holds what instances say to each other,
   with no I/O.** It hands out the bytes to sign and the bytes a signature
   must be of; the flows sign and verify through the `Signer` and
   `Verifier` ports, which already do Ed25519 on ring and on Web Crypto,
   so every host signs the same bytes.
2. **JSON is written one way, by RFC 8785.** Members are sorted by their
   names' UTF-16 code units, strings are escaped as ECMAScript's
   `JSON.stringify` escapes them, and numbers are written as ECMAScript
   writes a double. An integer that reads as a double written otherwise
   has no canonical form and is refused, so no two integers share one
   canonical text. JSON is read with exact float parsing, so a double
   reads back as the same double on every instance.
3. **Every herald object carries a W3C Data Integrity proof by the
   `eddsa-jcs-2022` cryptosuite**: a `DataIntegrityProof` naming the
   signing key as its `verificationMethod`, `assertionMethod` as its
   purpose, its creation time, and the Ed25519 signature of the SHA-256 of
   the canonical proof options followed by the SHA-256 of the canonical
   object, in base58btc. These are the bytes FEP-8b32 and Mastodon check,
   so the ActivityPub mirror's objects can carry the same proof. A relay
   or mirror passes the object and its proof on unchanged; the proof is
   the origin's, and each hop is told apart by its own HTTP signature.
4. **Every request and response between instances carries an RFC 9421
   HTTP message signature by Ed25519**, labelled and tagged `herald`, with
   `created`, `expires`, `keyid` and `alg` always, and a `nonce` on a
   request that changes anything. A body is covered through its RFC 9530
   `Content-Digest` (SHA-256; a receiver also checks SHA-512). A receiver
   finds herald's signature by its tag, refuses any other algorithm, and
   demands the parts, the nonce, the clock skew and the longest lifetime
   it names. Line breaks in a covered value are refused, so no value forges
   a line of the signature base.
5. **An instance publishes a key set**: its Ed25519 keys as Multikeys,
   newest first, under a `sequence` raised with each new set and a
   `refresh` hint in seconds. The newest key signs; every key in the set
   verifies, so a key is rotated by listing the new key before the old one
   for a while. An empty set withdraws every key. Each key's id is
   `https://<domain>/.well-known/herald#<publicKeyMultibase>`, used as a
   proof's verification method and as a signature's `keyid`; a set naming
   a key of another domain is refused.
6. **What a peer sends is bounded before it is parsed.** A signature
   field and a `Content-Digest` are read only up to the receiver's bound
   (8 KiB where it names none). A base58 value is refused when it is longer
   than the bytes it must decode to need, 34 for a Multikey and 64 for a
   signature, since decoding takes time growing with the square of its
   length. A document arrives within the limits herald already reads by:
   `[server] body` for what is posted to it, and each request's answer
   limit for what it fetches.
7. **A rotation statement is the new key set carrying a proof by a key
   the receiver pins**, with a sequence past the pinned set's. The receiver
   checks the proof against the pinned key it names; a statement by any
   other key, or with a sequence not past the pinned one, is refused.
   Whether a peer takes such statements at all is its `rotation` setting.

## Consequences

- herald implements RFC 8785, RFC 8941's dictionaries, RFC 9421 and
  RFC 9530 itself, each checked against its published examples, and the
  `eddsa-jcs-2022` proof against W3C's.
- A peer that pins a key keeps trusting the instance across rotations it
  accepts, without trusting what the network says about the instance's
  keys.
- The discovery document, the `herald` destination's feed and the peer
  endpoints build on these pieces.
