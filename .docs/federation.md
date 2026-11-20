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

`herald doctor` shows the instance among other heralds after its
findings: the domain, its keys newest first with their fingerprints, the
revoked keys, each peer with its URL, pinned keys, `rotation`, `rate`,
`most` and whether private events go to it, each feed and whether the
layer serves it, each `herald-push` with its inbox, and each `herald`
source with the feed it pulls and whether it takes pushes, and how long
the audit is kept. `--format json` carries the same as `federation`,
with each grant and every gap of it; `--online` also reads each peer's
discovery document and says whether its answer holds for the keys pinned
for it.

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
- A tombstone is checked like any envelope, and withdraws an identity the source took from every route taking the source as the route's `withdraw` says: only a post this source announced goes, never one another source announced under the same identity, and a route announcing each project as an actor of its own withdraws it as the actor of the project the cursor keeps for the identity. Its release entering again under a newer proof is announced anew.
- An envelope's identity is at most 1 KiB and its project at most 1 KiB. The cursor keeps every identity a pull's pages list, its proof and its project, every tombstone of an identity it took (at most `peer_share` of each instance it takes from: the peer, and with `verify = "hop"` each other peer) and the `tombstones` of others; herald.toml does not load where that cannot fit a record of the store, and where the source names none of `pages`, `tombstones` and `peer_share`, they are lowered to fit, as `herald doctor` names.

With `rotation = "signed"` on the peer, a key set its discovery document lists, proved by a key that stands for the peer under a later sequence and naming no revoked key, replaces the pinned keys while those stay pinned. A kept set naming a key `[federation] revoked` lists stands for nothing from the next pull on, and no revoked key ever verifies a page or an envelope. herald reads it again after the set's `refresh` (at most 7 days), and at once when an answer is signed by a key it does not trust, and keeps it in the store. `herald doctor` names every peer whose rotation is signed and, with `--online`, each set it took that herald.toml does not pin; `herald peer add --update` pins it. herald proves its own key set by its oldest key, so a peer pinning that key takes the newer ones.

## A static signed log

A publisher that runs herald only in CI serves its feeds from a static
host, such as GitHub Pages, as a signed log (ADR-0060):

```toml
[federation]
domain = "owner.github.io"
keys = [{ secret = "HERALD_FEDERATION_KEY" }]

[federation.pages]
url = "https://owner.github.io/repository"
```

- `herald poll`, `herald announce` and `herald withdraw` end by writing the log where a feed or the key set changed, or it was last written `refresh` ago (`"1d"`, at most `"7d"`), under the next sequence of a record the store keeps by compare-and-set; the markers store keeps none.
- `url`'s host must be `[federation] domain`, and its path is the instance's path: envelopes name `<url>/.well-known/herald`. Each feed's pages are fixed buckets of its page size in sequences, the newest at `herald/v1/feeds/<name>` and older ones at `herald/v1/pages/<name>/<bucket>`. `.well-known/herald` lists each page's URL and SHA-256 digest under the log's sequence and time, and `.well-known/herald.sig` holds the statement of its URL, digest, sequence and time, proved by the newest key. `.nojekyll` lets GitHub Pages serve `.well-known`.
- Each writing is one commit to `branch` (`herald-pages`) of `repository` (the run's own) with `token` (`GITHUB_TOKEN`), through `api` for a GitHub Enterprise Server, holding only the files that changed; the branch moves only where its head is still the commit's parent. Serve the branch with GitHub Pages, or any static host. `artifact` names a directory each writing fills with every file, for actions/upload-pages-artifact; named alone, no branch is written.
- A `herald` source reads it with `static = true`, by `url` and `keys` or by a peer whose `rotation` is `"manual"`. It takes the log only where the statement's proof holds for a key trusted for the instance, it names the document read and its digest, its sequence is no lower than the one read before, and it was written within `stale_after` (`"3d"`, at most `"30d"`; give more than the publisher's `refresh`); and each page only where its digest is the one listed. A withheld, replayed or rolled-back page, or a log no longer written, stops the pull and says so.

## Pushes into a source

A peer may push what it announces into a `herald` source rather than
wait for a pull, once both its table and the source grant it:

```toml
[peers.herald-example-net]
url = "https://herald.example.net"
keys = ["z6Mk…"]
grants = [{ resource = "sources.upstream", actions = ["push"], until = "2027-06-01T00:00:00Z" }]

[sources.upstream]
kind = "herald"
peers = { herald-example-net = { actions = ["push"], until = "2027-06-01T00:00:00Z" } }
```

- A grant names `actions` (`"push"`), `until` (an RFC 3339 string or a TOML datetime with its offset; never where absent) and `namespaces`, the identities the peer may give as they are, as a pulled source's `namespaces` does for its peer. herald.toml does not load unless the peer's `grants` and the source's `peers` name the same grant. A source naming neither `peer` and `feed` nor `url` takes only what the peers it grants push, and pins no keys of its own.
- The peer posts one envelope or tombstone to `POST /herald/v1/inbox/<source>`. herald keeps nothing of a request before one of its sender's rates counts it. It reads the body only once it carries an RFC 9421 signature by a key that stands for a peer over `@method`, `@target-uri`, `content-type` and `Content-Digest`, created and expiring within `[server] inbound.replay_window` of now, with a nonce, and the body is no larger than that peer's `most`; then the digest must name the body and the signature hold. Then its nonce is looked up, writing nothing: a nonce seen before counts only against the peer's replays (`[federation] replay_rate`, `"120/m"`, at most `"480/m"`), so a captured request replayed spends none of the peer's own rate, and a new one counts against the peer's `rate` however it is answered after, its nonce kept even where it is refused past the rate, so no signed request is taken twice before it expires. Two pushes racing with one nonce are taken once. Past either rate, `429`, and only the first of the window is audited. A peer has at most `[federation] refused_nonces` (2400, at most 9600) nonces of requests refused past its rate kept every 20 minutes; past them herald holds it, taking nothing of it and keeping nothing more until every request it made has expired, and audits the hold once. `herald doctor` names the setting wherever a peer is granted a push. The source must grant the peer `push` until later than now. The envelope is then taken as a pull of the peer takes it: under `herald/<peer>/`, but where the peer's grant gives its identity as it is, its proof holding for the instance that made it, newer than the source's, within `peer_share`. Its proof is kept in the source's cursor before herald answers `202`; its event is then announced, or its tombstone withdraws, as a pulled one would.
- The answers are fixed: `202` taken (and for an envelope no newer than the proof kept), `401` not signed by a peer's key, its signature not holding, or its nonce seen before, `403` not granted, the grant ended, or the envelope not one the source takes, `413` larger than the peer may send, `429` past the peer's `rate`, `503` the store cannot answer. Why goes to the log and the audit, never into the answer.
- Each signed request is kept in the audit of its peer's UTC day, or of `-` for one naming a key no peer holds or whose signature does not hold, within `[federation] stranger_rate` (`"120/m"`, at most `"480/m"`; the rest, and one with no readable signature, are only logged), so no one but the peer writes in its name or spends its rate: when, the peer or the key id it named, the action, the resource, the status and why, never its body; at most 1000 a day, the rest counted. `[federation] keep_audit` (`"90d"`, at most `"365d"`) is how long each day is kept. `herald peer log [name] [--since <day, time or 7d>] [--limit 1000] [--format json]` shows them newest first.
- `herald doctor` names what each peer may do with each resource and until when, each grant that never ends, and each that has ended or ends within `[federation] grants_ending` (`"7d"`, at most `"90d"`).
- An envelope a peer pushes names who may see its event: `public`, `internal` or `private`. The event is kept seen by no more than it names. A pulled feed's envelopes are public, and one naming any other is refused.

## Pushing to a peer

A route may push each event it takes into a peer's `herald` source, which grants this instance `push`:

```toml
[destinations.mirror]
kind = "herald-push"
peer = "herald-example-net"
inbox = "upstream"

[[routes]]
id = "mirror"
sources = ["releases"]
destination = "mirror"
```

- `peer` names a peer of `[peers]`; `inbox` the peer's source, whose inbox is `<url>/herald/v1/inbox/<inbox>`. `envelope` is the most bytes one envelope takes (`"16KiB"`, at most `"64KiB"`), the body cut to fit; `waits` how long it waits where the inbox answers that it is unwell or limited and names no wait.
- Each event is pushed as an envelope proved by the instance's newest key, in a request that key signs over `@method`, `@target-uri`, `content-type` and `Content-Digest`, with a nonce, lasting `[federation] signed_for`. `[federation]` must name the domain and keys, and the store must keep records: the route's ledger records each push, so an event is pushed once for each revision and an edit again. A wait the inbox asks for is kept, and any other refusal stops the route; either is tried again as any destination's step is.
- An event the routing sees as other than public is pushed only to a peer with `private = true`, its envelope naming who may see it; to any other peer it is left out, and withdrawn by a pushed tombstone where it was pushed before. A route declared private does not load to a peer taking no private events, unless `[doctor] private_to_public = "warn"`.
- A release withdrawn (`withdraw = "delete"`) is withdrawn from the peer by a pushed tombstone, and a re-created release pushed again.

## A CI run's ID token as its credential

A run of GitHub Actions or GitLab CI proves who it is with the ID token
its CI issues it, so neither the run nor its repository keeps a stored
token or key ([ADR-0064](decisions/0064-a-ci-run-s-id-token-is-its-credential.md)).
An app takes the tokens of the issuers its `oidc` entries name, for the
HTTP API and, standing for the names it lists in `peers`, for a herald
source's inbox:

```toml
[apps.plugins]
oidc = [{ issuer = "https://token.actions.githubusercontent.com", claims = { repository_owner_id = "123456", ref = "refs/tags/v*" } }]
peers = ["plugin-ci"]

[sources.plugins]
kind = "herald"
peers = { plugin-ci = { actions = ["push"], namespaces = ["github.com/123456"] } }
```

- `issuer` is compared with the token's `iss` exactly, before anything is fetched: `https://token.actions.githubusercontent.com` for GitHub Actions, with `/<enterprise>` after it where an enterprise of GitHub Enterprise Cloud issues its own; `https://token.actions.<enterprise>.ghe.com` on GHE.com; `https://<host>/_services/token` on GitHub Enterprise Server; `https://gitlab.com`, or the instance's URL on GitLab self-managed.
- `audience` is what the token's `aud` must name: `[server] url` where absent, else `https://` and `[federation] domain`. herald refuses an audience that is empty, an issuer, a peer's URL, or a GitHub's default audience, `<GitHub>/<owner>`, which every herald of the owner would take alike: `https://github.com/<owner>` always, `https://<enterprise>.ghe.com/<owner>` for a GHE.com issuer, and `https://<host>/<owner>` for a GitHub Enterprise Server issuer.
- `takes` is what the entry's tokens may do: `["announce"]` through the HTTP API, `["push"]` into a herald source as the app's `peers`, or both. Where absent it is what the app does; an app that names both projects and peers names it on each entry.
- `claims` maps a claim's name to the pattern its value must match, case and all, `*` and `?` never reaching past a `/`: `repository = "org/*"` takes `org/x` but not `org/a/b`, `org-evil/x` or `Org/x`. Any run may ask its CI for a token of any audience, so an entry must bind who the run is, or it would take a token of any repository at the issuer: one of `repository_id`, `repository_owner_id`, `project_id` and `namespace_id` with no wildcard at all, or one of `repository`, `repository_owner`, `project_path` and `namespace_path` with none before its first `/`, its owner. `sub` binds the run only in the shapes that name its repository, owner first: `repo:<owner>/...` (GitHub's default) or `project_path:<group>/...` (GitLab), the owner with no wildcard and the rest whatever the pattern says. On GitHub a repository's admin may shape its own `sub` from claims such as `ref`, `context` or `job_workflow_ref`, so a subject such as `ref:refs/heads/main`, `environment:prod` or `job_workflow_ref:org/shared/...` names no repository and binds no one. An entry binding only `ref`, `event_name`, `repository_visibility` or such a `sub`, or an owner such as `"org*"`, does not load. Prefer `repository_id` or `repository_owner_id` (GitLab: `project_id` or `namespace_id`): names, `sub` among them, are a names-only binding a renamed or re-created repository inherits, and `herald doctor` names an entry binding names alone as a gap. Every entry binding the claims a token carries takes it: the first app by name, and its first entry.
- `algorithms` (`["RS256"]`) is all a token may be signed with; the token's header never chooses on its own, and `none` and HMAC cannot be written.
- The token must name its expiry (`exp`) and its issue (`iat`), each in whole seconds, as must any `nbf`. It must not have expired, its `nbf` and `iat` must not lie ahead, give or take `skew` (`"1m"`, at most `"5m"`), and it must live no longer than `lifetime` from `iat` to `exp` (`"1h"`, at most `"1d"`), so a token minted to live for days is refused. GitLab CI's tokens live as long as their job's timeout; raise `lifetime` for a longer job.
- `replay` on the app says how often one of its tokens is taken. `"once"`, where the store keeps deliveries, is the default: each token taken is remembered among the store's deliveries, by its issuer and its `jti` (or, where it names none, the digest of its signed bytes; never the token), before anything it brings is taken, and refused a second time; the record outlives the token, as the store keeps deliveries for days. `"until-expiry"` takes a token as often as it comes until it expires. A store that keeps no deliveries, such as `markers`, takes each until it expires: herald does not load `replay = "once"` there, and `herald doctor` names it as a gap.
- What a captured token can do is therefore bounded. Under `"once"`, a token already presented can do nothing, and one captured before its run presented it can do once what that run could, to this herald alone and within `lifetime`. Under `"until-expiry"`, it can announce, or push as the run's name and within its grant, what it likes until it expires, though a push never overwrites an identity the source already holds: its proof is as of the token's `iat`. It can never reach another herald: its `aud` names this one, and a run never sends it elsewhere.
- The issuer's keys are read from the `jwks_uri` its discovery document (`<issuer>/.well-known/openid-configuration`) names, which must be `https` on the issuer's own origin, or from `jwks` where it is set; `keys` pins them, as a JSON Web Key Set's `keys` lists them, and nothing is read. herald reads only addresses the config names or the issuer's own origin, whatever a token says, follows no redirect, and reads at most 64 KiB of each; an issuer on a private network is reached because its operator named it. The keys are kept in the store, keyed by the issuer, and used for `keys_for` (`"1h"`, at most `"1d"`). A token naming a key they lack has them fetched again where the last fetch, or try, is older than `keys_retry` (`"1m"`, from `"10s"` to `keys_for`): however many tokens name unknown keys, the issuer is asked at most once in it, and a rotated key is taken within it. A fetch that fails leaves the kept keys in use until they are `keys_for` old, never longer. Keys kept as read later than now are read again, and a try kept as later than now is due, so a clock set back neither keeps old keys in use nor holds a fetch off. Every entry naming an issuer reads its keys alike. The store must keep records, unless the keys are pinned.
- A token that does not hold, or was taken before, is answered `401`; keys that cannot be read or kept, or a use that cannot be remembered, `503`. Each answer's body is herald's fixed text for its status; the reason is logged, and neither quotes the token.
- A name in `peers` is no table of `[peers]` and no other app's, and a herald source grants it in its own `peers` alone, as it grants a peer. No source grants one app two names, and `herald doctor` notes a name no source grants. A push presenting the token is taken as from that name: no larger than a peer's `most` (`"2MiB"`), within a peer's `rate` (`"120/m"`) in the name's own window, the source's grant of it holding, its identities outside the granted `namespaces` taken under `herald/<name>/`. Its envelope carries no proof; herald proves it with its own newest `[federation]` key, the first of `keys`, as made by itself in the name's stead: when it is taken, where the app takes each token once, so a run withdrawing what it pushed a moment before, with a token of its own, is newer than its push; as of the token's `iat` under `replay = "until-expiry"`, so a captured token presented again is never newer than what its run pushed. Each such push is kept in the name's audit, `herald peer log <name>`, with the key id `oidc <issuer>`, the envelope's identity as taken (under `herald/<name>/` where the grant's namespaces do not hold it) and its body's `Content-Digest`, and as its evidence the token's issuer, its `jti`, `sub`, `sha`, `run_id`, `run_attempt`, `job_workflow_ref`, `pipeline_id`, `job_id` and `ci_config_ref_uri` where it carries them, and each claim its entry binds; never the token.
- `herald doctor` lists each app's issuers and audiences, and the names it stands for, among the instance's federation.

A run pushing to its herald names the credential on its `herald-push`
destination:

```toml
[destinations.org]
kind = "herald-push"
peer = "org"
inbox = "plugins"
credential = "id-token"
```

- `credential = "id-token"` presents a CI run's ID token in place of `[federation]`'s key, which the run then needs none of. The envelope is pushed as it is, for the peer to prove.
- `id_token = "actions"`, the default, asks GitHub Actions for a fresh token for each push, meant for `audience`, with the job's `ACTIONS_ID_TOKEN_REQUEST_URL` and `ACTIONS_ID_TOKEN_REQUEST_TOKEN`; the job needs `permissions: id-token: write`, on github.com and GitHub Enterprise Server alike, and herald's Action needs nothing more. Each push presents a token of its own, so a peer taking each token once takes every push of the run.
- `id_token = "variables"` reads the tokens from `id_tokens` (`[{ secret = "HERALD_ID_TOKEN" }]` where absent), as GitLab CI's `id_tokens:` sets them, each with the destination's audience as its `aud`. Each token is presented for one push, in turn, so under a peer's `replay = "once"` a job pushes once for each token it holds: give one `id_tokens:` entry, and one line of `id_tokens`, for each push a job makes, such as `HERALD_ID_TOKEN_1` and `HERALD_ID_TOKEN_2`. Once every token was presented, the job's further pushes wait for a later run. Each destination names variables of its own.
- `audience` is what the token's `aud` must name: the peer's URL where absent, and herald does not load one on another origin than the peer's URL. herald reads each token before sending it and sends nothing where its `aud` names another, so a token meant for one herald is never presented to another.
