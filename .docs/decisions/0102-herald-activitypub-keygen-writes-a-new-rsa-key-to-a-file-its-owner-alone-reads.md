# ADR-0102: `herald activitypub keygen` writes a new RSA key to a file its owner alone reads

**Status:** Accepted
**Date:** 2026-11-27

## Context

An ActivityPub destination signs every delivery with an RSA private key
in PKCS#8 PEM, which Mastodon verifies
([ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md)
decision 7). ADR-0004 names `herald activitypub keygen` as the command that
writes a new key to a file, for the adopter to store as a secret
([ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md)).
ring, which signs herald's requests, makes no RSA key.

## Decision

1. **AWS-LC makes the key.** `herald-crypto`'s `keygen` feature makes it
   through `aws-lc-rs` (`rsa::KeyPair::generate`), with only its
   `aws-lc-sys` and `prebuilt-nasm` features, so no crate is held twice and
   `cargo deny` needs no exception. The native CLI alone turns the feature
   on: no wasi:http component, Fastly Compute service or JavaScript host
   links AWS-LC, and none of them runs the command.
2. **The key is read back before it is written.** herald's own signer reads
   the new key's public half from it, so a key herald could not sign with
   is never written, and the public key printed is the one the actor
   publishes.
3. **The file is written whole, for its owner alone where the system has
   modes.** The key goes first to a new file of its own beside the one
   named, made only where nothing is there, with mode `0600` on Unix, and
   synced. It is then linked in under the name asked, which fails where a
   file is there, or, with `--force`, renamed over it. The temporary file
   is removed either way. On Windows the new file takes the access list
   its folder gives a new file, which herald does not narrow: the command
   and its help say so there, in place of "readable by its owner alone".
4. **No part of the private key is printed.** The command prints the public
   key as PEM and where the private key is, and every line it writes passes
   through a scrubber that knows the private key. The key's PEM is built in
   a buffer zeroed when dropped.
5. **Its size is a setting.** `--bits` takes 2048, as Mastodon's own keys
   are and where absent, 3072 or 4096.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **RustCrypto's `rsa`** | Its private-key operations carry a timing advisory (RUSTSEC-2023-0071). |
| **Ed25519 keys alone** | Mastodon verifies RSA alone. |
| **Print the key to standard output** | The key would cross a terminal, a shell's history or a CI log. |
| **`aws-lc-rs` with its default features** | Its `ring-io` and `ring-sig-verify` features hold `untrusted` 0.7 beside ring's 0.9, which `cargo deny` refuses. |

## Consequences

- An adopter makes the actor's key with herald itself; `openssl genpkey`
  still makes one herald reads.
- The native build compiles AWS-LC's C sources, beside ring's.
- On Windows, who may read the key file is whoever its folder lets read a
  new file; the adopter checks it before storing the key.

## Related

- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): the ActivityPub destination and its key
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): secrets and the release's supply chain
- [ADR-0089](0089-herald-s-log-takes-a-level-and-a-format-and-every-secret-herald-resolves-or-sends-is-scrubbed.md): scrubbing every line herald writes
