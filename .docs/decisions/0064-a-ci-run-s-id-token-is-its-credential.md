# ADR-0064: a CI run's ID token is its credential

**Status:** Accepted
**Date:** 2026-11-20

## Context

An organization's repositories each run herald in CI and hand what they
announce to the organization's herald. Each such run proved who it was
with a stored secret: an app's token for the HTTP API (ADR-0015), or a
`[federation]` private key for a push into a herald source (ADR-0054).
Every repository kept a credential at rest, and any workflow of it could
read it. GitHub Actions and GitLab CI each issue a run a short-lived ID
token, a JWT signed by the CI's issuer, naming the repository, its
owner, the ref and the workflow; every host herald runs on verifies
RSASSA-PKCS1-v1_5 SHA-256, as the GitHub App's tokens are minted with it
(ADR-0041).

## Decision

1. **An ID token is a credential for the app API and the peer inbox.**
   `[apps.<name>] oidc` names the issuers whose tokens the app takes; a
   bearer shaped as a compact JWS is checked against them, any other
   against the app's hashed tokens. An app may stand for names, `peers`,
   which a herald source grants in its own `peers` as it grants a peer:
   a run presenting the token pushes into that source as the name. A
   CI repository's herald has no URL or keys, so the name is no table
   of `[peers]`; no name is a peer's or another app's, and no source
   grants one app two names, so a push names one grantee. Each entry
   says what its tokens may do, `takes`: `announce` through the API,
   `push` into an inbox, or both. Where absent it is what the app does;
   an app that both announces and stands for peers names it, so no token
   is taken by both unless its entry grants both.
2. **The token is checked in a fixed order.** It is read (at most 16 KiB),
   its `iss` compared exactly with every entry's issuer before anything
   is fetched; its header's `alg` must be in the entry's `algorithms`
   (`RS256` alone; `none` and HMAC cannot be written), its `aud` must
   name the entry's audience, and its `exp` and `iat`, which it must
   name, and any `nbf` must be whole seconds and hold within the entry's
   `skew` (`"1m"`, at most `"5m"`), its life from `iat` to `exp` no longer
   than the entry's `lifetime` (`"1h"`, at most `"1d"`). Then its
   signature is checked by the key its `kid` names in the issuer's key
   set, and every claim of the entry's `claims` must match its pattern.
   The first app by name, and its first entry, holding the token take it.
3. **An entry binds a run by patterns over any claim.** A pattern is
   anchored at both ends, case-sensitive, and cut at each `/` into as
   many parts as its claim, so `*` and `?` never reach past one:
   `org/*` takes `org/x` but not `org/a/b` or `org-evil/x`. As any run
   may ask for any audience, the config does not load an entry that
   binds no one: it must bind `repository_id`, `repository_owner_id`,
   `project_id` or `namespace_id` with no wildcard, or `repository`,
   `repository_owner`, `project_path` or `namespace_path` with none
   before its first `/`, the owner, or `sub` as `repo:<owner>/...` or
   `project_path:<group>/...` with the owner written whole. A GitHub
   repository's admin shapes its own `sub` from claims such as `ref`,
   `context` or `job_workflow_ref`, so any other `sub` binds no one.
   `repo:` and `project_path:` are the only subject shapes that count,
   whatever the issuer: that they name the repository holds for GitHub
   and GitLab, and for another issuer the operator must know whether a
   repository can shape its own `sub`. An
   entry binding `ref` alone, `sub = "environment:prod"`, or
   `repository_owner = "org*"`, would take a token of any repository at
   the issuer. `herald doctor` names an entry binding no id of the first
   four with no wildcard, ids a renamed or re-created repository never
   inherits, as a gap, `sub` alone included; the operator decides whether
   names suffice.
4. **The audience names one herald.** It is `[server] url`, or
   `https://` and `[federation] domain`, where the entry names none; the
   config refuses an audience that is empty, an issuer, the URL of a
   peer of `[peers]`, another herald's, or a GitHub's default
   `<GitHub>/<owner>`, which every herald of the owner would take alike:
   github.com's always, a GHE.com enterprise's for its issuer
   `https://token.actions.<enterprise>.ghe.com`, and a GitHub Enterprise
   Server's for its issuer `https://<host>/_services/token`.
5. **Only configured addresses are fetched.** The issuer's keys are read
   from the `jwks_uri` of its discovery document,
   `<issuer>/.well-known/openid-configuration`, which must name the
   issuer exactly and an `https` address on the issuer's own origin, or
   from the entry's `jwks`; `keys` pins them and nothing is read. Each
   issuer is reached through a connection of its own that reaches only
   its origin and its `jwks`, follows no redirect, and reads at most
   64 KiB within `[http]`'s bounds. Nothing a token says chooses an
   address, so no token makes herald reach a host the operator did not
   name; an issuer on a private network, such as GitHub Enterprise
   Server or GitLab self-managed, is reached because its operator named
   it there.
6. **Keys are kept per issuer and fetched again after a cooldown.** The
   key set is kept in the store under the issuer's digest, with when it
   was read and when it was last fetched or tried, and used for
   `keys_for` (`"1h"`, at most `"1d"`). A token naming a key the kept set
   lacks has it fetched again only where the last fetch, or try, is older
   than `keys_retry` (`"1m"`, from `"10s"` to `keys_for`). However many
   tokens name unknown keys, the issuer is asked at most once a cooldown,
   nothing a token names is written into the store, and a rotated key is
   taken within one cooldown. A fetch that fails leaves the kept keys in
   use until they are `keys_for` old, never longer; then the issuer's
   tokens are refused until a fetch holds. This cooldown replaces the
   first answer of one re-read per issuer per `keys_for`: under that, an
   unauthenticated request naming a made-up key could spend the window's
   one re-read just after a fetch, and a rotation within it would then
   lock every run out until the window ended. The algorithm, the issuer,
   the audience and the times are checked before the store is read or
   anything fetched, so a flood of tokens that fail them costs neither.
   Every entry naming an issuer reads its keys alike. The key set is
   public, so it is kept as read, its integrity resting on the store as a
   peer's kept key set's does (ADR-0049).
7. **A token is taken once, where the store can remember it.** Each
   app's `replay` is `"once"` where the store keeps deliveries: a token
   taken is admitted among the store's deliveries, under `oidc`, by its
   issuer's digest and its `jti`'s, or where it names none, the digest of
   its signed bytes and signature, never the token, after its signature
   and claims hold and before anything it brings is taken, and refused a
   second time. A crash after the admission leaves no second use. The
   store keeps deliveries for days, longer than any `lifetime`, so the
   record outlives the token. A request worked once more after it was
   answered, as the Lambda works it, takes its token again. A store that
   keeps no deliveries takes each token until it expires; the config
   refuses `"once"` written there, and `herald doctor` names it as a gap.
   `"until-expiry"` may be chosen anywhere, and doctor notes it. A
   captured token can then do this: under `"once"`, nothing once its run
   presented it, and once what its run could where it was captured before;
   under `"until-expiry"`, announce, or push as the run's name within
   the grant, what it likes until it expires, never overwriting an
   identity the source holds (decision 8). Either way it reaches this
   herald alone, as its `aud` names it, and within `lifetime`. This
   replaces the first answer, under which no `jti` was kept.
8. **What a run pushes is proved by the receiver.** The run's envelope
   carries no proof. The receiving herald makes its path itself, proves
   it with its own newest `[federation]` key, the first of `keys`, and
   takes it as from the name: within a peer's `rate` and `most` defaults
   in the name's own window, its identities outside the grant's
   `namespaces` under `herald/<name>/`. Where the app takes each token
   once, the proof is as of when herald takes the push, within the
   token's life, so a run's second push of one identity, such as a
   withdrawal of what it pushed, with a token of its own, is newer than
   its first; a proof as of `iat` would drop it. Where a token is taken
   until it expires, the proof is as of the token's `iat`, so a captured
   token presented again is never newer than what its run pushed, and
   cannot overwrite an identity the source holds. The name's audit keeps the push
   with the key id `oidc <issuer>`, the envelope's identity as taken and
   its body's `Content-Digest`, and as its evidence the issuer, the token's `jti`, `sub`,
   `sha`, `run_id`, `run_attempt`, `job_workflow_ref`, `pipeline_id`,
   `job_id` and `ci_config_ref_uri` where it carries them, and each claim
   the entry binds.
9. **The token is never kept or quoted.** No refusal, log line, trace,
   audit entry or kept record holds the token; a refusal's log line names
   the check that failed, `401` for a token that does not hold or was
   taken before and `503` for keys that cannot be read or kept or a use
   that cannot be remembered, and every answer's body is the fixed text
   of its status.
10. **A run presents a token of its own for each push, to the herald it
    is meant for alone.** `herald-push`'s `credential = "id-token"`
    presents a CI run's ID token in place of `[federation]`'s key. On
    GitHub Actions, `id_token = "actions"`, the default, asks for a fresh
    token for each push with the job's `ACTIONS_ID_TOKEN_REQUEST_URL` and
    `ACTIONS_ID_TOKEN_REQUEST_TOKEN`, meant for the destination's
    `audience`, so a run pushes as often as it needs under `"once"`. The
    route's connection reaches the peer and that URL's origin alone. On
    GitLab CI, whose tokens are fixed for the job, `id_token =
    "variables"` reads them from `id_tokens`, each presented for one push
    in turn, and only where its `aud` names the destination's audience,
    so one meant for another herald is left for it; a job pushes once
    for each, and the rest wait for a later run. Before sending, the run
    reads the token's `aud` and sends nothing unless it names the
    destination's `audience`, the peer's URL where absent and never
    another origin than it, so a token meant for one herald is never
    presented to another, which could present it again where it is
    meant.

## Consequences

- No CI run keeps a stored credential to announce or push; a repository's
  herald needs only the organization's herald's URL and pinned keys.
- Every enterprise issuer is a setting: GitHub Enterprise Cloud's
  `/<enterprise>` issuer, GHE.com's `token.actions.<enterprise>.ghe.com`,
  GitHub Enterprise Server's `_services/token`, GitLab self-managed, and
  any other OIDC issuer signing with RS256.
- A captured token is worth nothing once presented where the store keeps
  deliveries, and is worth what its run could do, to the one herald it
  names, until it expires where the store keeps none.
- ES256 and EdDSA tokens are refused until a host verifier for them
  exists; adding one is an `algorithms` value.
