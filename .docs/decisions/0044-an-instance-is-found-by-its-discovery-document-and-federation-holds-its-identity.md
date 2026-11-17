# ADR-0044: An instance is found by its discovery document, and `[federation]` holds its identity

**Status:** Accepted
**Date:** 2026-11-17

## Context

herald instances talk through a JSON API of herald's own, signed with
Ed25519 (ADR-0043). A peer must find an instance, read its keys and notice
when they change, and fediverse tooling should see what an instance runs.
Until now each ActivityPub destination held its own domain, RSA key and
follow policy, though they describe the instance rather than one
destination.

## Decision

1. **`[federation]` holds the instance's identity**: `domain`, `keys` (the
   secrets of its Ed25519 private keys, newest first; the first signs and
   every one verifies) and `refresh` ("1d" where absent, at most "7d").
   `keys` without a `domain` does not load. A run that serves reads the
   keys.
2. **An ActivityPub destination takes what it leaves out from
   `[federation]`**: `domain`, `key` (from `actor_key`), `mode`, `block`,
   `allow`, `followers`, `approve` and `lists`. What it names is its own,
   so one herald still serves actors on several domains. A destination
   kind declares the keys it takes, and the schema does not require them
   of the destination. With neither, the config names both places. The
   follow policy's types move to `herald-core` so `[federation]` and the
   destination share them.
3. **`/.well-known/herald` is the discovery document**, served on the
   instance's domain: the API version and base (`/herald/v1`), herald's
   name and version, the key set (ADR-0043) with `refresh`, and the
   features and feeds the instance offers.
4. **The store keeps the key set the instance last published**, and its
   sequence: the same keys keep their sequence, and any change, new keys,
   fewer or another order, raises it by one, written with compare-and-set
   within the store's write attempts. The markers store keeps none, so the
   set is published under sequence 0 and doctor names the gap, as peers
   take no new set at the same sequence.
5. **WebFinger finds the instance** as `acct:herald@<domain>` or by its
   domain's own URL, linking the discovery document, beside the actors it
   finds today.
6. **NodeInfo 2.1** is linked from `/.well-known/nodeinfo` and served at
   `/nodeinfo/2.1`: herald's name, version and repository, `activitypub`
   among its protocols only where a route announces as an actor on the
   domain, `usage.users.total` counting each such route, closed
   registrations, and the discovery document and feeds in its metadata.
7. **doctor** loads each instance key, wherever it is set, and names one
   that is no Ed25519 key; it names the markers store's gap and a layer
   with no endpoint, which serves no discovery document.
8. **Every host reads JSON with exact float parsing**, as `herald-flows`
   names `herald-peers`, which asks for it, so a double reads back as the
   same double wherever its canonical form is made.

## Consequences

- A peer finds an instance by its domain alone, pins its keys after the
  operator confirms them, and notices a rotation by the sequence.
- Signed feeds and the `herald` destination (the next step) add their
  feeds to the discovery document and NodeInfo.
- An existing ActivityPub destination loads unchanged.
