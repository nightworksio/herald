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
