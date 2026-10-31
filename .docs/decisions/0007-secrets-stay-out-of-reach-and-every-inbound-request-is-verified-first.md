# ADR-0007: Secrets stay out of argv, templates and logs, every inbound request is verified before it is parsed, and the supply chain is pinned and signed

**Status:** Accepted
**Date:** 2026-10-31

## Context

herald holds credentials that can post as a project to its community: a
Discord bot token, a Slack bot token, a Matrix access token, a Mastodon
account's token and an ActivityPub actor's private key. A leaked token posts
as the project, and so does a forged request herald trusts.

herald renders text it did not write. Release bodies, feed entries and app
events come from whoever can publish them. It also runs where its users'
secrets live: in their GitHub Actions, beside their repository token.

And herald is itself a dependency. A user pins its Action and runs its
binary with their secrets in the environment, so herald's own supply chain is
part of theirs.

## Decision

1. **Secrets come only through `Secrets`.**
   - **How a secret arrives.** The config names a secret and never holds it
     (ADR-0006). No secret is taken from argv, where process listings and
     shell history keep it. The Action passes secrets to the CLI through its
     environment.
   - **Where a secret never goes.** No secret enters a template's context.
   - **The `Secret` type.** A resolved secret has a type whose `Debug` and
     `Display` print its name and never its value. The type is the only way
     to hold one.
   - **Masking.** On Actions, herald masks every secret it resolves before its
     first output.
   - **Redaction.** Every error, log line and dry run redacts secrets by name,
     as `«secret:HERALD_DISCORD_TOKEN»`. Credentials that travel in a URL, a
     webhook's path or query, are redacted the same way.

2. **Event data never reaches a shell.**
   - **The Action.** `action.yml` hands the CLI the event through the file
     GitHub writes (`GITHUB_EVENT_PATH`) and inputs through the environment.
     No `${{ }}` expression places event data in a `run:` script.
   - **The CLI.** The CLI reads the event JSON itself.
   - **Spawning.** herald spawns no process with event data in its arguments.

3. **Inbound requests are verified before they are parsed.** The raw body
   and headers are checked first, and only a verified body reaches a parser:

   | Request | Verified by |
   |---------|-------------|
   | GitHub webhook | HMAC-SHA256 of the body (`X-Hub-Signature-256`), and the delivery id kept against replay |
   | Forgejo and Gitea webhook | HMAC-SHA256 of the body, and the delivery id kept against replay |
   | GitLab webhook | GitLab's signing token: HMAC-SHA256 over the delivery id, timestamp and body, inside a replay window. An instance that cannot sign its webhooks is served by polling instead, and doctor says so |
   | Discord interaction | Ed25519 over the timestamp and body, with the application's public key, inside a replay window |
   | Slack command or interaction | Slack's signing secret: HMAC-SHA256 over the version, timestamp and body, inside a five-minute replay window |
   | Matrix application-service transaction | the registration's homeserver token, compared in constant time |
   | ActivityPub activity | the HTTP signature, checked against the key of the actor that signed it, with the body's digest. An activity naming an actor other than the signer is refused |
   | HTTP API | a per-app bearer token (decision 4) |

   Every comparison of a secret or a signature takes constant time.

4. **App tokens are scoped and stored hashed.**
   - **Minting.** `herald token new --app <name>` prints a new 256-bit random
     token once, with the line the config keeps: the token's SHA-256 under
     `[apps.<name>]`.
   - **Scope.** The same table names the projects and routes the app may
     announce to.
   - **What a request may do.** A request whose token is unknown, or whose
     event falls outside its scope, is refused before its event is read.
   - **Why a fast hash.** The token is random and long, so it needs no slow
     password hash, and SHA-256 is fast enough to check on every request.

5. **What leaves herald is allow-listed.**
   - **Mentions.** Mentions are allow-listed per route. Discord's
     `allowed_mentions` names only the route's roles, and never `@everyone`
     or `@here`. Slack and Matrix mention only stored subscribers.
   - **Event text.** Event text is escaped for its dialect, so it cannot
     write a mention itself (ADR-0004, ADR-0006).
   - **Outbound hosts.**
     - Each destination reaches only its own hosts, and `Http` refuses any
       other (ADR-0004).
     - Every outbound request uses HTTPS. The one exception is a loopback base
       URL named explicitly in the config, as CI's fixture server is.
   - **Hosts herald reaches on others' behalf.** These are an ActivityPub
     actor's key and a follower's inbox. Each must resolve to a public
     address, so a crafted actor cannot point herald at its own network.
   - **Templates.** Template rendering is sandboxed: minijinja with no
     filesystem, a fuel limit and an output cap (ADR-0006).

6. **The supply chain is pinned, audited and signed.**
   - **Pinned.** Every action in every workflow is pinned by a full commit
     SHA, and every image by its digest. Dependabot moves them, after a
     seven-day cooldown.
   - **Audited.** `cargo-deny` checks advisories, licences, bans and sources,
     and refuses a second version of a crate unless the skip names a reason.
     Like lemonfiber's `deny.toml`, it bans crates that report on the person
     running them, and crates that run code they were handed. `cargo-audit`
     runs beside it.
   - **The workflows.** CodeQL analyses the workflows, the Action and the
     Rust code. zizmor audits the workflows and the Action, and OpenSSF
     Scorecard reports on the repository. gitleaks scans every push.
   - **Signed.** Releases are signed keylessly:
     - every binary, the checksums file and the wasm build carry GitHub's
       build provenance attestation;
     - the image is signed with cosign keyless, and attested;
     - `gh attestation verify` and `cosign verify` check them, and no
       long-lived signing key exists (ADR-0009).
   - **Checked by the Action.** The Action verifies each download's SHA-256
     against the release's checksums before it runs it.
   - **No unsafe code.** `unsafe` is forbidden in every crate.

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **Secrets in argv, as `--token`** | Process listings and shell history keep argv, and a CI log echoes a failing command line. |
| **Parse, then verify** | A parser bug would then be reachable by anyone who can send a request. Verifying the raw bytes first leaves the parsers to signed input alone. |
| **Bearer tokens kept in plain text** | Anyone who can read the config, or a backup of the store, could announce as every app. A hash lets herald check a token without being able to produce one. |
| **A long-lived release signing key** | It has to be generated, stored, rotated and never leaked. Keyless signing ties each artefact to the workflow and commit that built it, and `gh attestation verify` checks that with no key to distribute. |
| **Unrestricted outbound requests** | A config mistake or a crafted ActivityPub actor could make herald a proxy into its own network. |

## Consequences

**A dry run is safe to paste.** It prints every request, with every secret
redacted.

**A forged request reaches no parser.** The fuzzers prove the parsers on any
input anyway (ADR-0008).

**A user can verify what they run.** The Action checks the checksum, and the
attestation proves which workflow, at which commit, built the binary.

## Related

- [ADR-0003](0003-every-source-becomes-one-neutral-event.md): the parsers behind verification
- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): mentions, hosts and the ActivityPub actor
- [ADR-0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md): secret references and the template sandbox
- [ADR-0008](0008-every-line-is-covered-and-no-mutant-survives.md): fuzzing and the CI that runs these checks
- [ADR-0009](0009-herald-ships-as-binaries-an-action-an-image-and-a-wasm-build.md): how releases are signed and verified
