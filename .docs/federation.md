# herald among other instances

`[federation]` names the domain herald serves on and its Ed25519 keys,
newest first: the first signs, and every one verifies.

```toml
[federation]
domain = "herald.example.org"
keys = [{ secret = "HERALD_KEY" }]
```

Make a key with:

```sh
openssl genpkey -algorithm ed25519 -out herald-key.pem
```

`herald server` then answers `/.well-known/herald` on that domain: the API
it serves, its software and its key set, under a sequence the store raises
by one whenever the keys change. Peers pin the keys and read the set again
after `refresh` (`"1d"`, at most `"7d"`). Rotate by writing the new key
before the old one, and leave the old one out once peers know the new one.
WebFinger finds the instance as `acct:herald@herald.example.org`, and
NodeInfo 2.1 describes it at `/.well-known/nodeinfo`. herald reads at most
`fields` (`"8KiB"`, at most `"32KiB"`) of each signature field or
`Content-Digest` a peer sends, and refuses a longer one unread.

An ActivityPub destination takes `domain`, `key` (as `actor_key`) and its
follow policy (`mode`, `block`, `allow`, `followers`, `approve`, `lists`)
from `[federation]` where it leaves them out, and its own where it names
them. `herald doctor` checks that each key is an Ed25519 key, and names
the markers store, which keeps no key set, and a layer that serves no
endpoint.

## A feed of herald's own

A `herald` destination is a feed herald serves rather than posts to. Each
event a route takes is published as an envelope of what of it is public
(its identity, kind, project, title, body, links, the attributes its kind
declares, when it happened, and its forge, host and repository), proved by
the newest `[federation]` key, and listed newest first at
`/herald/v1/feeds/<destination>` on the federation domain:

```toml
[destinations.releases]
kind = "herald"

[[routes]]
id = "releases-feed"
sources = ["herald"]
destination = "releases"
```

- A route to a feed renders no template, so it names none.
- The feed keeps the newest `keep` envelopes (1000, at most 4000, fewer where the store's records hold fewer), lists `page` of them a page (20, at most 80), each `?before=<sequence>` the next, and tags each page for `If-None-Match`. An envelope takes at most `envelope` (`"16KiB"`, at most `"64KiB"`), and a longer event's body is cut, marked `"cut": true`.
- An edited or late Release enters again first, under a new sequence, in place of its older envelope.
- Only an event herald sees as public enters. Where a published event is no longer public, its envelope gives way to a tombstone. A route declared `private` to a feed does not load unless `[doctor] private_to_public = "warn"`.
- Every answer under `/herald/v1`, and the discovery document, carries an HTTP message signature by the newest key over its status, type, `Content-Digest`, tag, and the method and URL asked, valid for `[federation] signed_for` (`"5m"`, at most `"20m"`).
- The markers store keeps no feed, and a feed needs `[federation]`'s `domain` and `keys`. The discovery document and NodeInfo list each feed.

## Peers

A peer is another herald this one trusts, pinned by the keys its operator
confirms and never by what the network says:

```sh
herald peer add herald.example.net
```

reads the peer's discovery document, checks that its answer is signed by
a key it lists, shows each key's Multikey and `sha256:` fingerprint, and once you answer `y` writes the table into herald.toml,
keeping every other line:

```toml
[peers.herald-example-net]
url = "https://herald.example.net"
keys = ["z6Mk…"]
```

- Compare each fingerprint with one the peer's operator gave you. `--key <multikey or fingerprint>`, once for each key, pins only those keys and asks nothing; `--print` prints the table and writes nothing; `--name` names the peer (its domain with each `.` as `-` where absent); `--update` pins the keys a peer already named lists now.
- `herald peer list` shows each peer's keys and fingerprints. `herald peer test [name]` reads each peer's discovery document, checked against its pinned keys, and names a key it lists that is not pinned or a pinned key it no longer lists; it exits 1 where one fails or changed.
- `herald peer revoke <name>` removes the peer and adds its keys to `[federation] revoked`, and `--key` revokes one key and keeps the peer while it pins another. A peer pinning a revoked key does not load, nor does a key pinned for two peers.
- A peer is reached through `[http]`, as a forge herald is configured for is. Each answer must carry a signature by a pinned key over its status, type, `Content-Digest` and the URL asked, made and expiring within `[server] inbound.replay_window` of now and living at most 20 minutes, and its body is read only up to `most` (`"2MiB"`, at most `"8MiB"`). `herald peer test` takes the key set it lists only once that signature and its digest hold.
- `rotation` (`"manual"`, or `"signed"`), `rate` (`"120/m"`, at most `"480/m"`), `hops` (2, at most 8) and `private` (false) bound what a peer may send and receive.

## A peer's feed as a source

A `herald` source pulls a peer's feed, reading each page only once a key
pinned for the peer signed the answer, and each envelope only once its
proof holds:

```toml
[sources.upstream]
kind = "herald"
peer = "herald-example-net"
feed = "releases"

[[routes]]
id = "upstream-releases"
sources = ["upstream"]
destination = "discord"
template = "release"
```

- `peer` names a table of `[peers]`, and `feed` the feed its discovery document lists. An instance that is no peer is pulled with `url` (the feed's full URL) and `keys` (its Ed25519 Multikeys, given out of band) instead, within the bounds a peer takes where it says nothing.
- The first pull is quiet: it remembers where the feed stands and announces nothing, unless `backfill` names how many of the newest envelopes to announce (at most `pages` pages of 80). Later pulls ask with `If-None-Match` and walk `?before=` down to the newest sequence read, at most `pages` (5, at most 20) a pull.
- Every answer must be signed by a pinned key over its status, type, `Content-Digest` and the URL asked, as `herald peer test` checks, its body read only up to the peer's `most`. An answer that nothing changed (`304`) must be signed over its status, its `ETag` and the URL asked, its `ETag` the one asked for, and the cursor keeps an `ETag` only where the answer's signature covers it. Each envelope's `eddsa-jcs-2022` proof must hold for the instance that made it.
- An envelope is taken only where its proof was made later than the newest the source took of its identity, so a replayed or relayed envelope never edits, withdraws or brings back an event taken under a newer proof. A proof made more than `[server] inbound.replay_window` ahead of now is no proof: the envelope is passed over, its maker named in the poll's outcome and by `herald doctor`, and the pull reads on. The source forgets the proofs it took first, each instance's against its own even share of what the source remembers, never by the time a proof names and never because another instance took many, and never the proof of an identity it took an event of (not one it passed over) that a tombstone withdrew. A tombstone of any other identity withdraws nothing: its proof is kept, at most `peer_tombstones` (250, at most `tombstones`) of any one instance and `tombstones` (1000, at most 4000) in all, those taken first forgotten first, however many tombstones of identities it took any instance has it keep. Each instance may have the source keep at most `peer_share` (1000, at most 4000) tombstones of releases it withdrew: a pull takes no new identity of an instance at its share, names it in its outcome, and reads that envelope again once the share is raised, and `herald doctor` names each instance at its share. A tombstone of an identity the source took is always taken.
- `verify = "origin"` (the default) takes only envelopes the peer made itself. `verify = "hop"` also takes those it passes on from another peer of `[peers]`, proved by that peer's key, through at most the peer's `hops` instances and none twice.
- Each event's identity is taken as `herald/<peer>/<identity>` (`herald/<domain>/<identity>` for an instance pulled by `url`), so a peer cannot pre-empt another source's event, and its origin names the peer (forge `herald`, host the peer's domain), never the forge it claims; the claimed forge, host and repository are the attributes `claimed_forge`, `claimed_host` and `claimed_repository`. `namespaces` grants the identities the peer may give as they are, each the start of identities up to a `/` such as `"github.com/123456"`: an identity it grants keeps the host and repository its origin names only where they lie under the same namespace (`"github.com/acme"` grants `github.com/acme/gate/v1` with the origin `github.com` `acme/gate`), its forge still `herald`, as no envelope proves its forge; and only one the peer made itself is ever granted. An event of a kind this config does not declare, or not of its kind's shape, is passed over.
- A tombstone is checked like any envelope, and withdraws an identity the source took from every route taking the source as the route's `withdraw` says: only a post this source announced goes, never one another source announced under the same identity, and a route announcing each project as an actor of its own keeps its post. Its release entering again under a newer proof is announced anew.

With `rotation = "signed"` on the peer, a key set its discovery document lists, proved by a key that stands for the peer under a later sequence and naming no revoked key, replaces the pinned keys while those stay pinned. A kept set naming a key `[federation] revoked` lists stands for nothing from the next pull on, and no revoked key ever verifies a page or an envelope. herald reads it again after the set's `refresh` (at most 7 days), and at once when an answer is signed by a key it does not trust, and keeps it in the store. `herald doctor` names every peer whose rotation is signed and, with `--online`, each set it took that herald.toml does not pin; `herald peer add --update` pins it. herald proves its own key set by its oldest key, so a peer pinning that key takes the newer ones.
