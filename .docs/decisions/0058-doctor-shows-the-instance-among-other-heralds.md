# ADR-0058: doctor shows the instance among other heralds

**Status:** Accepted
**Date:** 2026-11-19

## Context

What herald.toml makes of an instance among other heralds is spread over
`[federation]`, `[peers]`, `herald` and `herald-push` destinations and
`herald` sources, and doctor named only what of it is wrong or costly. An
operator comparing keys with a peer, or checking what goes where, read
the config and its secrets by hand.

## Decision

1. **One section, in lines and in the JSON report.** Wherever the config
   names a domain, a key, a peer, a feed, a push or a `herald` source,
   `herald doctor` prints a federation section after its findings, and
   `--format json` carries it as `federation` (null where the config takes
   no part among other heralds), as `herald-doctor.schema.json` describes.
2. **What it shows.** The domain; the instance's keys, newest first, each
   with the secret holding it and, where that secret is set here and holds
   an Ed25519 key, its Multikey and fingerprint; each revoked key with its
   fingerprint; each peer with its URL, pinned keys and their
   fingerprints, `rotation`, `rate`, `most` and whether private events go
   to it; each feed with its URL and whether the layer serves it; each
   `herald-push` with its peer, the inbox it pushes into and whether
   private events go; each `herald` source with the feed it pulls and
   whether a peer may push into it; each grant with its actions and
   `until`; and `[federation] keep_audit`.
3. **Every gap of it, once more in its place.** The JSON section lists
   each gap whose key is `[federation]`'s or one of its peers',
   destinations' or sources'. The lines leave the gaps and grants to
   doctor's findings, which already name them.
4. **`--online` reads each peer.** Each peer's discovery document is read
   as `herald peer test` reads it, its answer checked against the keys
   pinned for the peer, and the section names whether it was read, and
   why not.

## Consequences

- An operator compares fingerprints with a peer from one command, and a
  script reads the same from the JSON report.
- A peer that cannot be reached, or answers with a key no longer pinned,
  shows in doctor before a pull or a push fails.
