# herald.toml settings

Every key `herald.toml` takes, generated from [`herald.schema.json`](../herald.schema.json). Regenerate it with `cargo run -p herald-compose --example settings > .docs/settings.md`.

## `announce`

How a run holds and keeps an announcement.

| Setting | Type | What it sets |
|---------|------|--------------|
| `announce.latest_attempts` | integer | How often a run writes what a project last announced again when another write lands between its read and its write; 3 where absent. |
| `announce.lease` | [Duration](#duration) | How long a run holds an announcement past its last write before another may finish it; "20m" where absent, longer than the longest a host lets one run go on, a Lambda's 15 minutes. |

## `apps`

The HTTP API's apps, by name.

| Setting | Type | What it sets |
|---------|------|--------------|
| `apps.<name>.projects` | array of string | The projects it may announce, at least one, each a glob ignoring case, such as `"plugin-*"`. A project named with no wildcard is one the app announces as; a glob only scopes what it may announce, and takes no project from a source or member. |
| `apps.<name>.routes` | array of string | The routes it announces to, each naming the app among its sources; every such route where it names none. |
| `apps.<name>.tokens` | array of [TokenHash](#tokenhash) | The SHA-256 of each of the app's tokens, written `sha256:` and 64 lowercase hex digits, as `herald token new` prints it. A token is rotated by adding the new one before the old one is removed. |

## `destinations`

The places herald posts to, by name.

### `destinations.<name>` of kind `discord`

Discord, by bot token or webhook.

| Setting | Type | What it sets |
|---------|------|--------------|
| `destinations.<name>` | table | Discord, by bot token or webhook. |
| `destinations.<name>.api` | string | Another base URL for Discord's API, such as a recorded-fixture server. |
| `destinations.<name>.archive_after` | [ArchiveAfter](#archiveafter) | How long a quiet thread stays open: `1h`, `1d`, `3d` or `1w`, the week where unset. |
| `destinations.<name>.colours` | table | The colours of its embeds. |
| `destinations.<name>.colours.prerelease` | [Colour](#colour) | A pre-release's colour; the violet of herald's banner, `#8f88c7`, where unset. |
| `destinations.<name>.colours.release` | [Colour](#colour) | A release's colour; the gold of herald's trumpet, `#e39a26`, where unset. |
| `destinations.<name>.interactions` | table | How it takes commands, where it takes any. |
| `destinations.<name>.interactions.application` | string | The application's id, from its General Information page. |
| `destinations.<name>.interactions.commands` | array of [Command](#command) | The commands it answers. |
| `destinations.<name>.interactions.domains` | array of string | Domains beside `discord.gg` a gateway session may connect under, such as a proxy's. |
| `destinations.<name>.interactions.group` | string | Registers the commands as subcommands of `/<group>`, such as `/herald latest`, in place of commands of their own. |
| `destinations.<name>.interactions.guild` | string | The server the commands are registered in alone, by id; where none is named, they are registered in every server the application is in. |
| `destinations.<name>.interactions.key` | string | The application's public key, in hex, from the same page. Each interaction posted to herald is checked against it. |
| `destinations.<name>.interactions.os` | string | The operating system the gateway session identifies as, in place of the host's own. |
| `destinations.<name>.interactions.prereleases` | boolean | Whether `latest` and `changelog` answer with a pre-release newer than every release. |
| `destinations.<name>.interactions.replies` | [Replies](#replies) | Who sees the answers to `latest` and `changelog`. |
| `destinations.<name>.interactions.template` | string | The template `latest` takes its title from and `changelog` its body, in place of the route's own. |
| `destinations.<name>.interactions.via` | [DiscordVia](#discordvia) | How Discord carries them. |
| `destinations.<name>.interactions.wording.<name>` | string |  |
| `destinations.<name>.kind` | string |  |
| `destinations.<name>.pin_lookups` | [PinLookups](#pinlookups) | How many pins it looks up, one request each, as it finds a route's earlier pins in a forum, from 1 to 100; 25 where unset. |
| `destinations.<name>.read_back` | [ReadBack](#readback) | How many pages of its own posts it reads back for their markers, from 1 to 20; 5 where unset. |
| `destinations.<name>.symbols` | table | The characters its posts write where its markup has none: a task done and to do, a rule and a text list's bullet. |
| `destinations.<name>.symbols.bullet` | [OneCharacter](#onecharacter) | A list's bullet where a list is written as text: `•` in Slack and `-` in plain text where unset. |
| `destinations.<name>.symbols.done` | [OneCharacter](#onecharacter) | A task done; `☑` where unset. |
| `destinations.<name>.symbols.rule` | [OneCharacter](#onecharacter) | A rule's character, drawn three wide; `─` where unset. |
| `destinations.<name>.symbols.todo` | [OneCharacter](#onecharacter) | A task to do; `☐` where unset. |
| `destinations.<name>.token` | table | The secret holding a bot token. A bot crossposts, threads, pins and reads back its posts. |
| `destinations.<name>.token.secret` | string | The secret's name. |
| `destinations.<name>.waits` | table | How long it waits before sending again where Discord answers that it is unwell or limited and names no wait. |
| `destinations.<name>.waits.limited` | [Duration](#duration) | After the platform answers that herald sends too often; the platform's own window where unset: "5m" on Mastodon, "5s" elsewhere. |
| `destinations.<name>.waits.outage` | [Duration](#duration) | After the platform answers that it is unwell; "5s" where unset. |
| `destinations.<name>.webhook` | table | The secret holding an incoming webhook's URL. A webhook only posts. |
| `destinations.<name>.webhook.secret` | string | The secret's name. |

### `destinations.<name>` of kind `mastodon`

An account on a server speaking the Mastodon client API.

| Setting | Type | What it sets |
|---------|------|--------------|
| `destinations.<name>` | table | An account on a server speaking the Mastodon client API. |
| `destinations.<name>.kind` | string |  |
| `destinations.<name>.read_back` | [ReadBack](#readback) | How many pages of its own posts it reads back for their markers, from 1 to 20; 5 where unset. |
| `destinations.<name>.server` | string | The server's base URL, such as `https://mastodon.social`. |
| `destinations.<name>.symbols` | table | The characters its posts write where its markup has none: a task done and to do, a rule and a text list's bullet. |
| `destinations.<name>.symbols.bullet` | [OneCharacter](#onecharacter) | A list's bullet where a list is written as text: `•` in Slack and `-` in plain text where unset. |
| `destinations.<name>.symbols.done` | [OneCharacter](#onecharacter) | A task done; `☑` where unset. |
| `destinations.<name>.symbols.rule` | [OneCharacter](#onecharacter) | A rule's character, drawn three wide; `─` where unset. |
| `destinations.<name>.symbols.todo` | [OneCharacter](#onecharacter) | A task to do; `☐` where unset. |
| `destinations.<name>.token` | table | The secret holding the account's access token, granted `write:statuses` and `read:statuses`. |
| `destinations.<name>.token.secret` | string | The secret's name. |
| `destinations.<name>.waits` | table | How long it waits before sending again where the server answers that it is unwell or limited and names no wait. |
| `destinations.<name>.waits.limited` | [Duration](#duration) | After the platform answers that herald sends too often; the platform's own window where unset: "5m" on Mastodon, "5s" elsewhere. |
| `destinations.<name>.waits.outage` | [Duration](#duration) | After the platform answers that it is unwell; "5s" where unset. |

### `destinations.<name>` of kind `matrix`

A bot account on a Matrix homeserver.

| Setting | Type | What it sets |
|---------|------|--------------|
| `destinations.<name>` | table | A bot account on a Matrix homeserver. |
| `destinations.<name>.homeserver` | string | The homeserver's base URL, such as `https://matrix.org`. |
| `destinations.<name>.interactions` | table | How it takes commands, where it takes any. |
| `destinations.<name>.interactions.appservice_token` | table | The secret holding the token herald acts with as the application service (`as_token`). |
| `destinations.<name>.interactions.appservice_token.secret` | string | The secret's name. |
| `destinations.<name>.interactions.commands` | array of [Command](#command) | The commands it answers. |
| `destinations.<name>.interactions.homeserver_token` | table | The secret holding the token the homeserver presents with each transaction it pushes (`hs_token`). |
| `destinations.<name>.interactions.homeserver_token.secret` | string | The secret's name. |
| `destinations.<name>.interactions.prefix` | string | What a message starts with to be a command to herald: `!herald latest php-mutation-gate`. |
| `destinations.<name>.interactions.prereleases` | boolean | Whether `latest` and `changelog` answer with a pre-release newer than every release. |
| `destinations.<name>.interactions.rate_limited` | boolean | Whether the homeserver holds the application service's user to its rate limits, as its registration says; true where unset. |
| `destinations.<name>.interactions.replies` | [Replies](#replies) | Whether answers to `latest` and `changelog` are posted to the room; Matrix has no answer seen by one member alone, so `ephemeral` answers in a thread under the command. |
| `destinations.<name>.interactions.sender` | string | The application service's user, by its localpart. |
| `destinations.<name>.interactions.sync_wait` | [Duration](#duration) | How long a sync waits for a message before it answers with none, "20s" where unset; shorter than `[http] total`, which bounds every answer. |
| `destinations.<name>.interactions.template` | string | The template `latest` takes its title from and `changelog` its body, in place of the route's own. |
| `destinations.<name>.interactions.timeline` | integer | The most messages one sync reads from each room, from 1 to 100; 20 where unset. |
| `destinations.<name>.interactions.via` | [MatrixVia](#matrixvia) | How the homeserver carries them. |
| `destinations.<name>.interactions.wording.<name>` | string |  |
| `destinations.<name>.kind` | string |  |
| `destinations.<name>.pin_lookups` | [PinLookups](#pinlookups) | How many of the room's pins it looks up, one request each, as it finds a route's earlier pins, from 1 to 100; 25 where unset. |
| `destinations.<name>.read_back` | [ReadBack](#readback) | How many pages of its own posts it reads back for their markers, from 1 to 20; 5 where unset. |
| `destinations.<name>.symbols` | table | The characters its posts write where its markup has none: a task done and to do, a rule and a text list's bullet. |
| `destinations.<name>.symbols.bullet` | [OneCharacter](#onecharacter) | A list's bullet where a list is written as text: `•` in Slack and `-` in plain text where unset. |
| `destinations.<name>.symbols.done` | [OneCharacter](#onecharacter) | A task done; `☑` where unset. |
| `destinations.<name>.symbols.rule` | [OneCharacter](#onecharacter) | A rule's character, drawn three wide; `─` where unset. |
| `destinations.<name>.symbols.todo` | [OneCharacter](#onecharacter) | A task to do; `☐` where unset. |
| `destinations.<name>.token` | table | The secret holding the bot account's access token. |
| `destinations.<name>.token.secret` | string | The secret's name. |
| `destinations.<name>.waits` | table | How long it waits before sending again where the homeserver answers that it is unwell or limited and names no wait. |
| `destinations.<name>.waits.limited` | [Duration](#duration) | After the platform answers that herald sends too often; the platform's own window where unset: "5m" on Mastodon, "5s" elsewhere. |
| `destinations.<name>.waits.outage` | [Duration](#duration) | After the platform answers that it is unwell; "5s" where unset. |

### `destinations.<name>` of kind `slack`

Slack, by bot token or webhook.

| Setting | Type | What it sets |
|---------|------|--------------|
| `destinations.<name>` | table | Slack, by bot token or webhook. |
| `destinations.<name>.api` | string | Another base URL for Slack's API, such as a recorded-fixture server. |
| `destinations.<name>.domain` | [Domain](#domain) | The Slack the workspace lives on: `slack.com`, Slack's own and an Enterprise Grid's, where unset, or `slack-gov.com` for Slack's government cloud. |
| `destinations.<name>.interactions` | table | How it takes commands, where it takes any. |
| `destinations.<name>.interactions.app` | table | The secret holding an app-level token (`xapp-…`) with `connections:write`, which opens Socket Mode connections. |
| `destinations.<name>.interactions.app.secret` | string | The secret's name. |
| `destinations.<name>.interactions.command` | string | The slash command members give, followed by a command and a project: `/herald latest php-mutation-gate`. |
| `destinations.<name>.interactions.commands` | array of [Command](#command) | The commands it answers. |
| `destinations.<name>.interactions.domains` | array of string | Domains beside its Slack's own a Socket Mode session may connect under, such as a proxy's. |
| `destinations.<name>.interactions.name` | string | The name its app and bot user go by in the manifest `herald setup` prints: up to 35 lowercase letters, digits, `.`, `-` or `_`, and `herald` where unset. |
| `destinations.<name>.interactions.prereleases` | boolean | Whether `latest` and `changelog` answer with a pre-release newer than every release. |
| `destinations.<name>.interactions.replies` | [Replies](#replies) | Who sees the answers to `latest` and `changelog`. |
| `destinations.<name>.interactions.signing` | table | The secret holding the app's signing secret, which each request posted to herald is checked against. |
| `destinations.<name>.interactions.signing.secret` | string | The secret's name. |
| `destinations.<name>.interactions.template` | string | The template `latest` takes its title from and `changelog` its body, in place of the route's own. |
| `destinations.<name>.interactions.via` | [SlackVia](#slackvia) | How Slack carries them. |
| `destinations.<name>.interactions.wording.<name>` | string |  |
| `destinations.<name>.kind` | string |  |
| `destinations.<name>.pin_lookups` | [PinLookups](#pinlookups) | How many of its own pins it looks up, one request each, as it finds a route's earlier pins, from 1 to 100; 25 where unset. |
| `destinations.<name>.read_back` | [ReadBack](#readback) | How many pages of its own posts it reads back for their markers, from 1 to 20; 5 where unset. |
| `destinations.<name>.symbols` | table | The characters its posts write where its markup has none: a task done and to do, a rule and a text list's bullet. |
| `destinations.<name>.symbols.bullet` | [OneCharacter](#onecharacter) | A list's bullet where a list is written as text: `•` in Slack and `-` in plain text where unset. |
| `destinations.<name>.symbols.done` | [OneCharacter](#onecharacter) | A task done; `☑` where unset. |
| `destinations.<name>.symbols.rule` | [OneCharacter](#onecharacter) | A rule's character, drawn three wide; `─` where unset. |
| `destinations.<name>.symbols.todo` | [OneCharacter](#onecharacter) | A task to do; `☐` where unset. |
| `destinations.<name>.team` | string | The workspace's id, such as `T0123456789`, where the token is an Enterprise Grid org-wide app's: each Web API call names it. |
| `destinations.<name>.token` | table | The secret holding a bot token (`xoxb-…`). A bot threads, pins and reads back its posts. |
| `destinations.<name>.token.secret` | string | The secret's name. |
| `destinations.<name>.waits` | table | How long it waits before sending again where Slack answers that it is unwell or limited and names no wait. |
| `destinations.<name>.waits.limited` | [Duration](#duration) | After the platform answers that herald sends too often; the platform's own window where unset: "5m" on Mastodon, "5s" elsewhere. |
| `destinations.<name>.waits.outage` | [Duration](#duration) | After the platform answers that it is unwell; "5s" where unset. |
| `destinations.<name>.webhook` | table | The secret holding an incoming webhook's URL. A webhook only posts. |
| `destinations.<name>.webhook.secret` | string | The secret's name. |

### `destinations.<name>` of kind `activitypub`

herald's own ActivityPub actors, on its domain.

| Setting | Type | What it sets |
|---------|------|--------------|
| `destinations.<name>` | table | herald's own ActivityPub actors, on its domain. |
| `destinations.<name>.allow` | array of string | The servers and actors an `allowlist` federates with, written as `block` writes them; `[federation]`'s where unset. |
| `destinations.<name>.approve` | [Approve](#approve) | Who takes a follow: `auto`, herald by the lists and caps, or `manual`, the operator through `herald followers`; `[federation]`'s where unset. |
| `destinations.<name>.block` | array of string | The servers and actors its actors never take a follow from or deliver to: each a domain, naming its subdomains too, or an actor's id; `[federation]`'s where unset. |
| `destinations.<name>.deliveries` | integer | How many of an announcement's followers' inboxes it delivers to at once, by every command that announces; 8 where unset. |
| `destinations.<name>.domain` | string | The domain herald serves its actors on, such as `herald.example`: a host alone, with no scheme, port or path; `[federation] domain` where unset. |
| `destinations.<name>.followers` | table | How many may follow each actor, in all and from any one server; a follow past either is answered with a `Reject`; `[federation]`'s where unset. |
| `destinations.<name>.followers.most` | integer | The most followers one actor keeps; no limit where unset. |
| `destinations.<name>.followers.per_domain` | integer | The most followers one actor keeps from any one server; no limit where unset. |
| `destinations.<name>.key` | table | The secret holding the actors' RSA private key, as PKCS#8 PEM, such as `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048` writes; `[federation] actor_key` where unset. |
| `destinations.<name>.key.secret` | string | The secret's name. |
| `destinations.<name>.kind` | string |  |
| `destinations.<name>.lists[].file` | string | The file, as a path beside `herald.toml`. |
| `destinations.<name>.lists[].sha256` | string | The SHA-256 of the file's text, in hex: a list that changes is not read until its new digest is pinned. |
| `destinations.<name>.mode` | [Mode](#mode) | Whom its actors federate with: `open`, everyone `block` does not name, or `allowlist`, only those `allow` names; `[federation]`'s where unset. |
| `destinations.<name>.outbox_page` | integer | How many Notes one page of each actor's outbox holds, from 1 to 80; 20 where unset. |
| `destinations.<name>.symbols` | table | The characters its posts write where its markup has none: a task done and to do, a rule and a text list's bullet. |
| `destinations.<name>.symbols.bullet` | [OneCharacter](#onecharacter) | A list's bullet where a list is written as text: `•` in Slack and `-` in plain text where unset. |
| `destinations.<name>.symbols.done` | [OneCharacter](#onecharacter) | A task done; `☑` where unset. |
| `destinations.<name>.symbols.rule` | [OneCharacter](#onecharacter) | A rule's character, drawn three wide; `─` where unset. |
| `destinations.<name>.symbols.todo` | [OneCharacter](#onecharacter) | A task to do; `☐` where unset. |
| `destinations.<name>.waits` | table | How long it waits before sending again where an inbox answers that it is unwell or limited and names no wait. |
| `destinations.<name>.waits.limited` | [Duration](#duration) | After the platform answers that herald sends too often; the platform's own window where unset: "5m" on Mastodon, "5s" elsewhere. |
| `destinations.<name>.waits.outage` | [Duration](#duration) | After the platform answers that it is unwell; "5s" where unset. |

### `destinations.<name>` of kind `herald`

A feed of herald's own, served on the [federation] domain: each event as an envelope proved by the instance's newest key.

| Setting | Type | What it sets |
|---------|------|--------------|
| `destinations.<name>` | table | A feed of herald's own, served on the [federation] domain: each event as an envelope proved by the instance's newest key. |
| `destinations.<name>.envelope` | [Size](#size) | The most bytes one envelope takes, its proof counted: "16KiB" where unset, and at most "64KiB". A longer event's body is cut to fit. |
| `destinations.<name>.keep` | integer | The most envelopes the feed keeps, newest first: 1000 where unset, fewer where the store's records hold fewer, and at most 4000. |
| `destinations.<name>.kind` | string |  |
| `destinations.<name>.page` | integer | The most envelopes a page lists: 20 where unset, and at most 80. |

## `doctor`

How doctor judges the config.

| Setting | Type | What it sets |
|---------|------|--------------|
| `doctor.private_kinds` | [Guard](#guard) | What becomes of a route that takes a private kind's events, such as `alert`'s, and says nothing of whether its place is private: "refuse" where absent, "warn" to load it and have doctor name it, or "allow". |
| `doctor.private_to_public` | [Guard](#guard) | What becomes of a route that takes a private or internal repository's events and is not `private`: "refuse" where absent, so the config does not load and no such event reaches it; "warn" to load it and have doctor name it; or "allow". |

## `federation`

This herald among other instances: its domain, its keys, and what its ActivityPub actors take by default.

| Setting | Type | What it sets |
|---------|------|--------------|
| `federation.actor_key` | table | The secret holding the RSA private key herald's ActivityPub actors sign with, as PKCS#8 PEM. An ActivityPub destination that names no `key` takes it. |
| `federation.actor_key.secret` | string | The secret's name. |
| `federation.allow` | array of string | The servers and actors an `allowlist` federates with, for an ActivityPub destination that names no `allow`. |
| `federation.approve` | [Approve](#approve) | Who takes a follow, for an ActivityPub destination that names no `approve`: `auto` or `manual`. |
| `federation.block` | array of string | The servers and actors herald's actors never take a follow from or deliver to, for an ActivityPub destination that names no `block`. |
| `federation.domain` | string | The domain herald serves its discovery document and actors on, such as `herald.example`: a host alone, with no scheme, port or path. An ActivityPub destination that names no `domain` takes it. |
| `federation.fields` | [Size](#size) | The most bytes a peer's signature field, such as `Signature` or `Content-Digest`, may hold and still be read: "8KiB" where absent, at most "32KiB". A longer one is refused before it is parsed. |
| `federation.followers` | table | How many may follow each actor, for an ActivityPub destination that names no `followers`. |
| `federation.followers.most` | integer | The most followers one actor keeps; no limit where unset. |
| `federation.followers.per_domain` | integer | The most followers one actor keeps from any one server; no limit where unset. |
| `federation.keys[].secret` | string | The secret's name. |
| `federation.lists[].file` | string | The file, as a path beside `herald.toml`. |
| `federation.lists[].sha256` | string | The SHA-256 of the file's text, in hex: a list that changes is not read until its new digest is pinned. |
| `federation.mode` | [Mode](#mode) | Whom herald's actors federate with, for an ActivityPub destination that names no `mode`: `open` or `allowlist`. |
| `federation.refresh` | [Duration](#duration) | How long a peer may keep the instance's key set before it reads it again; "1d" where absent, at most "7d". |
| `federation.revoked` | array of string | Peer keys that are never trusted again, each a Multikey such as `z6Mk…`, as `herald peer revoke` adds them: a peer pinning one does not load, and a new key set naming one is refused. |
| `federation.signed_for` | [Duration](#duration) | How long each response of herald's API stays signed, from when it is made: "5m" where absent, at most "20m". |

## `github_apps`

The GitHub Apps sources read as, by name.

| Setting | Type | What it sets |
|---------|------|--------------|
| `github_apps.<name>.api` | string | The REST API the App's tokens are minted at; `https://api.github.com` where absent, such as `https://ghe.example/api/v3` on GitHub Enterprise Server or `https://api.example.ghe.com` on GHE.com. Each source naming the App reads that host. |
| `github_apps.<name>.app_id` | integer | The App's id, as its settings page shows it; give it or `client_id`. |
| `github_apps.<name>.client_id` | string | The App's client id, such as `Iv23liABCDEF`, which GitHub recommends over the id; give it or `app_id`. |
| `github_apps.<name>.private_key` | table | The App's private key, as GitHub hands it out (PKCS#1) or as PKCS#8. |
| `github_apps.<name>.private_key.secret` | string | The secret's name. |
| `github_apps.<name>.store_token` | boolean | Whether each installation token is kept in the store between runs, so a run skips minting one while it is good; false where absent. The token, good for an hour at most, is then a credential at rest: herald keeps it encrypted with AES-256-GCM under a key derived from `private_key`, bound to its record and its expiry, in a record the store seals too. |
| `github_apps.<name>.webhook` | table | The secret the App's webhook deliveries are signed with, taken at `/hooks/app/<name>`. |
| `github_apps.<name>.webhook.secret` | string | The secret's name. |

## `http`

How herald's own requests reach the services the config names.

| Setting | Type | What it sets |
|---------|------|--------------|
| `http.body` | [Size](#size) | The most bytes of a response's body herald reads; "32MiB" where absent, and at most "128MiB". A host reached on others' behalf is read no further than "32MiB" whatever this says. |
| `http.ca_file` | string | A PEM file of the certificate authorities herald's own requests trust beside the Mozilla roots, read beside the config, such as a GitHub Enterprise Server's private CA. |
| `http.connect` | [Duration](#duration) | How long a connection may take to open; "10s" where absent. |
| `http.proxy` | string | The proxy herald's own requests go through: "environment", the host's `HTTPS_PROXY` and `NO_PROXY` where it honours them; "none"; or a proxy's http or https URL. "environment" where absent. |
| `http.total` | [Duration](#duration) | How long a request may take, from sending it to the end of its answer; "30s" where absent. `HERALD_TIMEOUT_MS` takes its place. |

## `kinds`

The kinds of event the config declares beside those herald ships, by name; its apps may announce them.

| Setting | Type | What it sets |
|---------|------|--------------|
| `kinds.<name>.attributes.<name>.required` | boolean | Whether every event of the kind carries it. |
| `kinds.<name>.attributes.<name>.type` | [AttributeType](#attributetype) | What it holds. |
| `kinds.<name>.description` | string | What it is, in a sentence. |

### `kinds.<name>.links[]`: A link whose button is labelled by its name

| Setting | Type | What it sets |
|---------|------|--------------|
| `kinds.<name>.links[]` | string | A link whose button is labelled by its name. |

### `kinds.<name>.links[]`: A link and its button's label

| Setting | Type | What it sets |
|---------|------|--------------|
| `kinds.<name>.links[]` | table | A link and its button's label. |
| `kinds.<name>.links[].label` | string | Its button's label. |
| `kinds.<name>.links[].name` | string | Its name, as `event.links.<name>` reads it. |

| Setting | Type | What it sets |
|---------|------|--------------|
| `kinds.<name>.private` | boolean | Whether its events belong in private places only, so a route taking them must say whether its place is private (`[doctor] private_kinds`). |
| `kinds.<name>.versioned` | boolean | Whether its events carry a version, as `attributes.version`, and may be pre-releases, as `attributes.prerelease`. |

## `markers`

The installation's marker key.

| Setting | Type | What it sets |
|---------|------|--------------|
| `markers.key` | table | The secret holding the marker key, such as `HERALD_MARKER_SECRET`. |
| `markers.key.secret` | string | The secret's name. |

## `peers`

The other heralds this one trusts, by name.

| Setting | Type | What it sets |
|---------|------|--------------|
| `peers.<name>.hops` | integer | The most instances an object from the peer may have passed through: 2 where absent, at most 8. |
| `peers.<name>.keys` | array of string | The peer's Ed25519 keys, each a Multikey as its discovery document lists it, such as `z6Mk…`, pinned by `herald peer add` once the operator confirms them. A key is never taken from the network unless a pinned key signed it. |
| `peers.<name>.most` | [Size](#size) | The most bytes of a body the peer sends that are read: "2MiB" where absent, at most "8MiB". A longer one is refused before it is parsed. |
| `peers.<name>.private` | boolean | Whether private events may go to the peer; false where absent. |
| `peers.<name>.rate` | [Rate](#rate) | How many requests the peer may make of this herald: "120/m" where absent, at most "480/m". |
| `peers.<name>.rotation` | [Rotation](#rotation) | Whether a new key set the peer signs with a pinned key replaces the pinned keys: `manual` (the operator pins each new key) where absent, or `signed`. |
| `peers.<name>.url` | string | Where the peer is served, such as `https://herald.example`: `https://`, its domain, and the path its endpoints are under where they are not at the root. Its discovery document is `/.well-known/herald` under it. |

## `render`

How far a template may go as it renders.

| Setting | Type | What it sets |
|---------|------|--------------|
| `render.fuel` | integer | The fuel one render may burn; 100000 where absent, and at most 400000. |
| `render.output` | [Size](#size) | The most it may write; "256KiB" where absent, and at most "1MiB". |
| `render.recursion` | integer | How deep its loops and calls may nest; 16 where absent, and at most 64. |

## `reports`

The reports routes name among their sources, by name.

| Setting | Type | What it sets |
|---------|------|--------------|
| `reports.<name>.at` | string | The wall-clock time a day's first window begins at, as "HH:MM": "09:00" where absent. |
| `reports.<name>.days` | array of [Day](#day) | The days windows begin on, every day where empty. |
| `reports.<name>.every` | [Duration](#duration) | How long each window lasts within a day, from "1m" to "1d": "1d" where absent. |
| `reports.<name>.view` | string | The view it shows: "queue", every source's open pull requests, where absent. |
| `reports.<name>.zone` | string | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam": "UTC" where absent. |

## `routes`

Which events go where, rendered how.

### `routes[]`: A route to a Discord destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].channel` | string | The channel's id. A bot needs one; a webhook posts where it was made. |
| `routes[].crosspost` | boolean | Whether to crosspost each announcement. |
| `routes[].destination` | string | The destination it announces to. |
| `routes[].edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `routes[].forum` | boolean | Whether the channel is a forum, where each announcement is a post of its own. |
| `routes[].group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `routes[].group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `routes[].group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `routes[].group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `routes[].group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `routes[].group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `routes[].group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `routes[].group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `routes[].id` | string | The route's stable id, part of every announcement's key. |
| `routes[].kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `routes[].late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pinning](#pinning) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].subscribe` | table | The opt-in role members subscribe to, where the route offers one. |
| `routes[].subscribe.button` | boolean | Whether each announcement carries a button that subscribes, or unsubscribes a member already subscribed. |
| `routes[].subscribe.role` | string | The opt-in role `subscribe` gives a member and `unsubscribe` takes away, by id. The route mentions it with its announcements. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed, which publishes each event as an envelope. |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route to a Mastodon destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].content_warning` | string | The content warning each status is shown behind. |
| `routes[].crosspost` | boolean | Whether to crosspost each announcement. |
| `routes[].destination` | string | The destination it announces to. |
| `routes[].edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `routes[].group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `routes[].group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `routes[].group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `routes[].group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `routes[].group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `routes[].group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `routes[].group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `routes[].group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `routes[].id` | string | The route's stable id, part of every announcement's key. |
| `routes[].kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `routes[].language` | string | The language each status is written in, as an ISO 639 code. |
| `routes[].late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `routes[].limit` | integer | The most characters the server takes in a status, Mastodon's own 500 where it is not given. |
| `routes[].link_length` | integer | The characters the server counts a link as, however long it is: its `characters_reserved_per_url`, Mastodon's own 23 where unset. |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pinning](#pinning) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed, which publishes each event as an envelope. |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].title` | integer | The longest title, a status's first line, in characters; 100 where unset. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].visibility` | [Visibility](#visibility) | Who sees each status. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route to a Matrix destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].crosspost` | boolean | Whether to crosspost each announcement. |
| `routes[].destination` | string | The destination it announces to. |
| `routes[].edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `routes[].group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `routes[].group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `routes[].group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `routes[].group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `routes[].group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `routes[].group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `routes[].group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `routes[].group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `routes[].id` | string | The route's stable id, part of every announcement's key. |
| `routes[].kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `routes[].late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pinning](#pinning) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].room` | string | The room's id, such as `!abcdefg:matrix.org`, which the bot has joined. |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed, which publishes each event as an envelope. |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route to a Slack destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].channel` | string | The channel's id, such as `C0123456789`, which the bot has joined. A bot needs one; a webhook posts where it was made. |
| `routes[].crosspost` | boolean | Whether to crosspost each announcement. |
| `routes[].destination` | string | The destination it announces to. |
| `routes[].edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `routes[].group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `routes[].group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `routes[].group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `routes[].group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `routes[].group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `routes[].group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `routes[].group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `routes[].group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `routes[].id` | string | The route's stable id, part of every announcement's key. |
| `routes[].kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `routes[].late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pinning](#pinning) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].subscribe` | table | How the route offers subscribing, where it mentions its subscribers. |
| `routes[].subscribe.button` | boolean | Whether each announcement carries a button that subscribes, or unsubscribes a member already subscribed. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed, which publishes each event as an envelope. |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route announcing as one of herald's own ActivityPub actors

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].actor` | string | The actor the route announces as, such as `php-mutation-gate`, which is followed as `@php-mutation-gate@<domain>`; or a template naming each project's own actor from `event.project`, such as `{{ event.project }}`. A project whose name is no actor's name is not announced on the route. |
| `routes[].content_warning` | string | The content warning each Note is shown behind. |
| `routes[].crosspost` | boolean | Whether to crosspost each announcement. |
| `routes[].destination` | string | The destination it announces to. |
| `routes[].edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `routes[].group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `routes[].group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `routes[].group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `routes[].group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `routes[].group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `routes[].group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `routes[].group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `routes[].group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `routes[].id` | string | The route's stable id, part of every announcement's key. |
| `routes[].kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `routes[].language` | string | The language each Note is written in, as an ISO 639 code. |
| `routes[].late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pinning](#pinning) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed, which publishes each event as an envelope. |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].visibility` | [Audience](#audience) | Who sees each Note on the timelines. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route publishing its events to a feed of herald's own

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].crosspost` | boolean | Whether to crosspost each announcement. |
| `routes[].destination` | string | The destination it announces to. |
| `routes[].edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `routes[].group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `routes[].group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `routes[].group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `routes[].group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `routes[].group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `routes[].group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `routes[].group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `routes[].group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `routes[].id` | string | The route's stable id, part of every announcement's key. |
| `routes[].kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `routes[].late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pinning](#pinning) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed, which publishes each event as an envelope. |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

## `run`

How many routes and sources a run goes through at once.

| Setting | Type | What it sets |
|---------|------|--------------|
| `run.routes` | integer | How many routes an event is announced to at once; 4 where absent, and 1 announces to one route after another in the file's order. |
| `run.sources` | integer | How many sources a poll reads at once; 4 where absent, and 1 reads one source after another. |

## `schedule`

How often a driver that keeps its own schedule polls and keeps house.

| Setting | Type | What it sets |
|---------|------|--------------|
| `schedule.housekeep` | [Duration](#duration) | How often threads are kept. |
| `schedule.poll` | [Duration](#duration) | How often sources are polled. |

## `server`

How `herald server` listens, and what it takes on at once.

| Setting | Type | What it sets |
|---------|------|--------------|
| `server.accept_pause` | [Duration](#duration) | How long the server waits after a connection could not be accepted; "100ms" where absent. |
| `server.base` | string | The path every endpoint is served under, for a reverse proxy that serves herald beside other things, as "/herald"; the root where absent. herald's ActivityPub actors' ids are under it too. |
| `server.body` | [Size](#size) | The most an inbound request's body may hold; "1MiB" where absent, and at most "4MiB". |
| `server.busy_retry` | [Duration](#duration) | How long a request whose work found no place is asked to wait, in its `Retry-After`; "30s" where absent. |
| `server.completions` | integer | How many autocompletions a session answers at once; one arriving while as many are answered is dropped. 4 where absent. |
| `server.connections` | integer | How many connections it holds at once; 256 where absent. |
| `server.fetches` | integer | How many actor documents it reads at once; 4 where absent. |
| `server.held` | [Duration](#duration) | How long a connection is held at most, its one request answered; "90s", or `read` where that is longer, where absent; at least `read`. |
| `server.inbound` | table | What a signed inbound request may carry and still be checked. |
| `server.inbound.covered` | integer | The most headers one ActivityPub signature may cover; 16 where absent, at least the 4 it must cover, and at most 64. |
| `server.inbound.replay_window` | [Duration](#duration) | How far a signed timestamp may lie from now, either way, before the request is taken for a replay; "5m" where absent, and at most "20m". |
| `server.inbound.signatures` | integer | The most signatures a request may carry, each checked in turn; 4 where absent, and at most 16. |
| `server.jobs` | integer | How many pieces of work it does at once, each an announcement, an activity or a command a request leaves; 8 where absent. |
| `server.keys` | table | The keys inbound activities are checked with. |
| `server.keys.cache` | [KeyCache](#keycache) | Where they are kept: "memory", the default, or "store". |
| `server.keys.document` | [Size](#size) | The most of a key's or an actor's document read; "256KiB" where absent, and at most "1MiB". |
| `server.keys.most` | integer | The most kept in memory, the one used longest ago forgotten first; 1024 where absent. |
| `server.keys.refresh_after` | [Duration](#duration) | How long a key is used before its document is read again; 1h where absent. |
| `server.keys.reread_after` | [Duration](#duration) | How long after a key is read an activity that does not hold under it is refused rather than having it read again; 1m where absent. |
| `server.listen` | string | The address and port it listens on; `--listen` and `HERALD_LISTEN` take its place. "127.0.0.1:8080" where absent, for a reverse proxy in front of it. |
| `server.queue` | integer | How many pieces of work may wait for a place; a request leaving one while as many wait is answered 503. As many as `jobs` where absent. |
| `server.read` | [Duration](#duration) | How long a connection may take to send a request's headers; "30s" where absent. |
| `server.sessions` | table | How a session a destination holds waits before it connects again. |
| `server.sessions.first_wait` | [Duration](#duration) | The first wait after a session fails to connect; "1s" where absent. |
| `server.sessions.jitter` | boolean | Whether each wait is between half of it and all of it, so sessions that failed together do not all connect again at once; true where absent. |
| `server.sessions.longest_wait` | [Duration](#duration) | The longest wait between attempts; "1m" where absent, and at least `first_wait`. |
| `server.sessions.stale` | [Duration](#duration) | How long ago a command a session taken up again answers may have been given; one given earlier is skipped and logged; "10m" where absent. |
| `server.silence` | [Duration](#duration) | How long a session a destination holds hears nothing before it checks the connection is alive, and then as long again before it opens another; "30s" where absent. |
| `server.url` | string | herald's public address as forges and platforms reach it, such as `https://herald.example`, with `base` after it where one is given: what a webhook herald registers points at. A setup command's `--url` takes its place. |
| `server.work` | table | How the work a request leaves keeps within its host's limits. |
| `server.work.after` | [Duration](#duration) | How long the work may run after the answer; where absent, Cloudflare Workers' 30 seconds of `waitUntil` there, and unbounded on every other host. |
| `server.work.requests` | integer | The most outbound requests a request and the work it leaves may make, counted from when herald began answering it; where absent, Fastly Compute's 32 backend requests there (set 10 on a trial), and unbounded on every other host. |
| `server.work.spare` | [Duration](#duration) | How much of `after` must be left for a send to start; "10s" where absent, and less than `after`. |
| `server.work.spare_requests` | integer | How many of `requests` must be left for a send to start: the send, and the writes after it where the store writes over HTTP; 4 where absent, and fewer than `requests`. |

## `sources`

What herald listens to, by name.

### `sources.<name>` of kind `github`

A GitHub repository's releases, tags and packages.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A GitHub repository's releases, tags and packages. |
| `sources.<name>.alerts` | table | Which security alerts are announced where the signals take "alert": what raised them, what they did and how severe they are; every tool's raised and reopened alerts where absent. |
| `sources.<name>.alerts.actions` | array of [AlertAction](#alertaction) | What an alert did that is announced: `created`, `reopened`, `confirmed` (GitLab), `fixed`, `dismissed`; `["created", "reopened"]` where absent. |
| `sources.<name>.alerts.severities` | array of [Severity](#severity) | How severe an alert is for it to be announced; every alert, those with no severity too, where empty. |
| `sources.<name>.alerts.tools` | array of [Tool](#tool) | What raised the alerts announced; every tool the forge raises alerts with where absent. |
| `sources.<name>.api` | string | The API's base URL, for a forge that is not GitHub, GitLab.com or Codeberg: a GitHub Enterprise Server's `https://ghe.example/api/v3`, a GHE.com tenant's `https://api.acme.ghe.com`, a self-managed or Dedicated GitLab's `https://gitlab.example/api/v4`, or a Forgejo's `https://forge.example/api/v1`. Where absent, it is the CI run's own when announcing the run's repository, and otherwise the forge's public instance. An owner is listed at the same base. |
| `sources.<name>.app` | string | The GitHub App the source reads as, by the name of its `[github_apps.<name>]` table, in place of `token`: herald mints an installation token for each round. |
| `sources.<name>.archived` | boolean | Whether archived repositories are taken. |
| `sources.<name>.backfill` | integer | How many of the newest releases, tags and package versions the first poll announces; none where absent (ADR-0003). For an owner, it applies to the members the first listing finds; a member found later announces only what it publishes after it was found. |
| `sources.<name>.discovery` | [Discovery](#discovery) | How an owner source finds the repositories that join or leave it: listing the owner each round ("list"), or only where it holds no listing, its selection changed or a delivery said the owner's repositories changed ("push"); "list" where absent. |
| `sources.<name>.edits_depth` | integer | How many of the newest Releases a poll watches for an edit of their name or notes, where a route taking the source edits them (`edits = "release"`); 30 where absent, and at most `page`. |
| `sources.<name>.exclude` | array of string | The names left out, as `include` writes them, whatever `include` says. |
| `sources.<name>.feed_page` | integer | How many activities one page of a Forgejo owner's feed asks for under `transport = "feed"`; 50 where absent, and at most 200. |
| `sources.<name>.feed_pages` | integer | How many pages of a Forgejo owner's feed a round reads back before it polls every member; 3 where absent, and at most 12. |
| `sources.<name>.fork` | boolean | Whether forks are taken. |
| `sources.<name>.graphql` | string | The GraphQL endpoint of a GitHub or GitLab owner source; beside `api` where absent: `/api/graphql` on a GitHub Enterprise Server whose `api` ends in `/api/v3` and on GitLab, whose `api` ends in `/api/v4`, and `/graphql` on GitHub and GHE.com. |
| `sources.<name>.graphql_batch` | integer | How many members one GraphQL request asks for; 50 where absent, and at most 100. |
| `sources.<name>.include` | array of string | The names of the owner's repositories taken, as globs in which `*` stands for any run of characters, `?` for any one and `[...]` for any one of a class, ignoring case; every name where absent. |
| `sources.<name>.installation` | integer | The id of the App's installation tokens are minted for; looked up on the owner, or on the repository, where absent. |
| `sources.<name>.issues` | table | Which issues are announced where the signals take "issue": what they did, and their labels and authors; every issue opened, closed or reopened where absent. |
| `sources.<name>.issues.actions` | array of [IssueAction](#issueaction) | What an issue did that is announced: `opened`, `closed`, `reopened`; every one where absent. |
| `sources.<name>.issues.authors` | array of string | The authors, as globs, whose issues are announced; every author where empty. |
| `sources.<name>.issues.confidential` | boolean | Whether GitLab's confidential issues are announced, each seen as private, so only a route declared private takes it; false where absent. |
| `sources.<name>.issues.labels` | array of string | The labels, as globs as `include` writes them, any one of which an issue announced carries; every issue where empty. |
| `sources.<name>.keep_members` | [Duration](#duration) | How long a member that left the selection keeps its cursor before it is forgotten; "30d" where absent. |
| `sources.<name>.key` | string | The field each entry of the member file names its repository by; "name" where absent. |
| `sources.<name>.kind` | string |  |
| `sources.<name>.language` | array of string | Languages, any one of which a member is written in; any where absent. |
| `sources.<name>.listed_page` | integer | How many repositories one page of the listing asks for; 100 where absent on GitHub and GitLab, and 50 on Forgejo, whose instances answer at most 50 unless configured otherwise. |
| `sources.<name>.member_hooks` | [MemberHooks](#memberhooks) | Who registers a project webhook on each member of a GitLab owner, whose group cannot send its own on GitLab Free: no one ("off"), `herald hooks <name>` ("setup"), or each poll round too ("round"); "off" where absent. Each hook points at `[server] url` and is signed with `webhook`. |

### `sources.<name>.members` of kind `github`: A file beside the config, such as `repos.toml`

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>.members` | table | A file beside the config, such as `repos.toml`. |
| `sources.<name>.members.file` | string | Its path, from the config's directory. |

### `sources.<name>.members` of kind `github`: A file in a repository on the source's forge, read through its API with the source's token each round

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>.members` | table | A file in a repository on the source's forge, read through its API with the source's token each round. |
| `sources.<name>.members.path` | string | The file's path in it. |
| `sources.<name>.members.ref` | string | The branch, tag or commit it is read at; the default branch where absent. |
| `sources.<name>.members.repository` | string | The repository, as `repository` writes one. |

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>.most_listed` | integer | The most repositories a listing reads before the filters; 10000 where absent. A listing over it refuses the source for the round. |
| `sources.<name>.most_members` | integer | The most members the filters may keep; 1000 where absent, and at most 100000. A selection over it refuses the source for the round. |
| `sources.<name>.most_packages` | integer | The most entries one listing of packages reads; 1000 where absent, and at most 100000. A listing over it refuses the source for the round. |
| `sources.<name>.moving` | array of string | The container tags that move from one image to the next, which are never announced, as globs as `include` writes them; `["latest"]` where absent. |
| `sources.<name>.owner` | string | The organization, GitLab group or user whose repositories are the source's members, each polled on its own, in place of `repository`. |
| `sources.<name>.package_types` | array of string | The types of package announced where the signals take "package", as the forge names them, such as `["container", "npm"]`; every type where absent. GitHub's are npm, maven, rubygems, docker, nuget and container. |
| `sources.<name>.package_visibility` | [PackageVisibility](#packagevisibility) | Who may see a package's version: the stricter of the package's own visibility and its repository's ("stricter"), or the package's own ("package"); "stricter" where absent. A version not everyone may see reaches only `private` routes under `[doctor] private_to_public = "refuse"`. |
| `sources.<name>.packages` | array of string | The packages announced, as globs as `include` writes them; every package where absent. |
| `sources.<name>.page` | integer | How many of the newest releases, and of the newest tags, one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB" where absent, and at most four times that. |
| `sources.<name>.prefer` | [Prefer](#prefer) | Which of a tag and its Release is announced where a repository's signals take both: the Release, a tag with none waiting for it as long as `tag_window` ("release"), or whichever comes first ("first"); "release" where absent. Holding a tag needs a store that keeps held events, which the markers store does not. |
| `sources.<name>.prerelease` | [PrereleaseRule](#prereleaserule) | How a pre-release is told: by the forge's flag or a pre-release part in the version ("either"), by the flag alone ("flag"), or by the version alone ("tag"); "either" where absent. A tag carries no flag, and neither does a GitLab release. |
| `sources.<name>.project` | string | The project a member's events are announced as, a template of `repository.name`, `repository.path`, `repository.owner`, `repository.id`, `repository.topics`, `repository.properties` and `repository.visibility`; the repository's name where absent. |
| `sources.<name>.properties.<name>` | [Values](#values) | A property's values: one, or any of several. |
| `sources.<name>.pulls` | table | Which pull requests are announced where the signals take "pull-request": what they did, whether drafts are, and their labels and authors; every pull request opened, ready for review or merged where absent. |
| `sources.<name>.pulls.actions` | array of [PullAction](#pullaction) | What a pull request did that is announced: `opened`, `ready_for_review`, `reopened`, `merged`, `closed`; `["opened", "ready_for_review", "merged"]` where absent. |
| `sources.<name>.pulls.authors` | array of string | The authors, as globs, whose pull requests are announced, such as `dependabot[bot]`; every author where empty. |
| `sources.<name>.pulls.drafts` | boolean | Whether a draft's opening is announced; false where absent, so a draft is announced once it is ready for review. |
| `sources.<name>.pulls.labels` | array of string | The labels, as globs as `include` writes them, any one of which a pull request announced carries; every pull request where empty. |
| `sources.<name>.registry` | string | The host of the container registry an image is named on, as in `registry.example:5000`: where absent, ghcr.io beside GitHub's API, `containers.<host>` beside a GitHub Enterprise Server's or a GHE.com tenant's, each image's own on GitLab, and the instance's on Forgejo. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.repositories.<name>.prerelease` | [PrereleaseRule](#prereleaserule) | How a pre-release of theirs is told, as the source's `prerelease` writes it; the source's where absent. |
| `sources.<name>.repositories.<name>.project` | string | The project their events are announced as, a template as the source's `project` is; the source's where absent. |
| `sources.<name>.repositories.<name>.routes` | array of string | The routes their events also reach, by id, whatever sources those routes name; each route's other keys still narrow what it takes. |
| `sources.<name>.repositories.<name>.routes_only` | boolean | Whether `routes` are the only routes their events reach, leaving out the routes that name the source; false where absent. |
| `sources.<name>.repositories.<name>.signals` | array of [Signal](#signal) | What they announce, as the source's `signals` writes it; the source's where absent. |
| `sources.<name>.repository` | string | The repository, as `owner/name`, or a GitLab project's full path, such as `group/subgroup/name`; or `owner` in its place. |
| `sources.<name>.runs` | table | Which runs are announced where the signals take "run": what they are read from, what they did, how they ended, and their branches, triggers and workflows; every finished run of the forge's own CI where absent. |
| `sources.<name>.runs.actions` | array of [RunAction](#runaction) | What a run did that is announced: `requested`, `in_progress`, `completed`; `["completed"]` where absent. |
| `sources.<name>.runs.branches` | array of string | The branches or tags runs are announced of, as globs as `include` writes them; every one where empty. One name with no glob in it is asked of the forge, so a poll reads only its runs. |
| `sources.<name>.runs.conclusions` | array of [Conclusion](#conclusion) | How a finished run ended for it to be announced; every conclusion where empty. |
| `sources.<name>.runs.from` | array of [RunFrom](#runfrom) | What runs are read from: "actions", the forge's own CI, and on GitHub "checks", the check suites and commit statuses other CI services report; `["actions"]` where absent. |
| `sources.<name>.runs.triggers` | array of string | What started the runs announced, as the forge names it, such as `push` or `schedule`; GitLab's `merge_request_event` is written `pull_request`. Every trigger where empty. |
| `sources.<name>.runs.workflows` | array of string | The workflows, pipelines, check suites' apps or status contexts whose runs are announced, as globs as `include` writes them; every one where empty. |
| `sources.<name>.signals` | array of [Signal](#signal) | What the source announces of each repository: its published Releases, its version tags, the versions of its packages, or several; `["release"]` where absent. A tag and a Release of it share one identity, so each route announces it once. |
| `sources.<name>.subgroups` | boolean | Whether a GitLab group's members include the projects of its subgroups; true where absent. |
| `sources.<name>.tables` | array of string | The arrays of the member file that list repositories, such as `["repo", "ungoverned"]`; the file's own array where absent, which only JSON can hold. |
| `sources.<name>.tag_window` | [Duration](#duration) | How long a tag with no Release waits for one, counted from when herald first sees the tag; "1h" where absent, and at most "7d". |
| `sources.<name>.template` | boolean | Whether template repositories are taken. |
| `sources.<name>.token` | table | A token that reaches a private repository or raises the API's rate limit, such as the run's `GITHUB_TOKEN`. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.topics` | array of string | Topics, any one of which a member holds; any where absent. |
| `sources.<name>.transport` | [Transport](#transport) | How an owner's members are polled; "auto" where absent. GitHub offers auto, graphql, rest and events; GitLab auto, graphql and rest; Forgejo auto, rest and feed. |
| `sources.<name>.visibility` | array of [RepositoryVisibility](#repositoryvisibility) | The visibilities taken; only public repositories where absent. A repository whose visibility the forge does not name is private. A route that takes a private or internal member is refused unless it is `private` (`[doctor] private_to_public`). |
| `sources.<name>.webhook` | table | The secret the forge signs its webhook deliveries to `herald server` with: GitHub's and Forgejo's webhook secret, or GitLab's signing token (ADR-0007). An owner source takes its repositories' and its organization's deliveries, and announces one only where the owner's last membership record holds the repository when its work runs. |
| `sources.<name>.webhook.secret` | string | The secret's name. |

### `sources.<name>` of kind `gitlab`

A GitLab project's releases, tags and packages.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A GitLab project's releases, tags and packages. |
| `sources.<name>.alerts` | table | Which security alerts are announced where the signals take "alert": what raised them, what they did and how severe they are; every tool's raised and reopened alerts where absent. |
| `sources.<name>.alerts.actions` | array of [AlertAction](#alertaction) | What an alert did that is announced: `created`, `reopened`, `confirmed` (GitLab), `fixed`, `dismissed`; `["created", "reopened"]` where absent. |
| `sources.<name>.alerts.severities` | array of [Severity](#severity) | How severe an alert is for it to be announced; every alert, those with no severity too, where empty. |
| `sources.<name>.alerts.tools` | array of [Tool](#tool) | What raised the alerts announced; every tool the forge raises alerts with where absent. |
| `sources.<name>.api` | string | The API's base URL, for a forge that is not GitHub, GitLab.com or Codeberg: a GitHub Enterprise Server's `https://ghe.example/api/v3`, a GHE.com tenant's `https://api.acme.ghe.com`, a self-managed or Dedicated GitLab's `https://gitlab.example/api/v4`, or a Forgejo's `https://forge.example/api/v1`. Where absent, it is the CI run's own when announcing the run's repository, and otherwise the forge's public instance. An owner is listed at the same base. |
| `sources.<name>.app` | string | The GitHub App the source reads as, by the name of its `[github_apps.<name>]` table, in place of `token`: herald mints an installation token for each round. |
| `sources.<name>.archived` | boolean | Whether archived repositories are taken. |
| `sources.<name>.backfill` | integer | How many of the newest releases, tags and package versions the first poll announces; none where absent (ADR-0003). For an owner, it applies to the members the first listing finds; a member found later announces only what it publishes after it was found. |
| `sources.<name>.discovery` | [Discovery](#discovery) | How an owner source finds the repositories that join or leave it: listing the owner each round ("list"), or only where it holds no listing, its selection changed or a delivery said the owner's repositories changed ("push"); "list" where absent. |
| `sources.<name>.edits_depth` | integer | How many of the newest Releases a poll watches for an edit of their name or notes, where a route taking the source edits them (`edits = "release"`); 30 where absent, and at most `page`. |
| `sources.<name>.exclude` | array of string | The names left out, as `include` writes them, whatever `include` says. |
| `sources.<name>.feed_page` | integer | How many activities one page of a Forgejo owner's feed asks for under `transport = "feed"`; 50 where absent, and at most 200. |
| `sources.<name>.feed_pages` | integer | How many pages of a Forgejo owner's feed a round reads back before it polls every member; 3 where absent, and at most 12. |
| `sources.<name>.fork` | boolean | Whether forks are taken. |
| `sources.<name>.graphql` | string | The GraphQL endpoint of a GitHub or GitLab owner source; beside `api` where absent: `/api/graphql` on a GitHub Enterprise Server whose `api` ends in `/api/v3` and on GitLab, whose `api` ends in `/api/v4`, and `/graphql` on GitHub and GHE.com. |
| `sources.<name>.graphql_batch` | integer | How many members one GraphQL request asks for; 50 where absent, and at most 100. |
| `sources.<name>.include` | array of string | The names of the owner's repositories taken, as globs in which `*` stands for any run of characters, `?` for any one and `[...]` for any one of a class, ignoring case; every name where absent. |
| `sources.<name>.installation` | integer | The id of the App's installation tokens are minted for; looked up on the owner, or on the repository, where absent. |
| `sources.<name>.issues` | table | Which issues are announced where the signals take "issue": what they did, and their labels and authors; every issue opened, closed or reopened where absent. |
| `sources.<name>.issues.actions` | array of [IssueAction](#issueaction) | What an issue did that is announced: `opened`, `closed`, `reopened`; every one where absent. |
| `sources.<name>.issues.authors` | array of string | The authors, as globs, whose issues are announced; every author where empty. |
| `sources.<name>.issues.confidential` | boolean | Whether GitLab's confidential issues are announced, each seen as private, so only a route declared private takes it; false where absent. |
| `sources.<name>.issues.labels` | array of string | The labels, as globs as `include` writes them, any one of which an issue announced carries; every issue where empty. |
| `sources.<name>.keep_members` | [Duration](#duration) | How long a member that left the selection keeps its cursor before it is forgotten; "30d" where absent. |
| `sources.<name>.key` | string | The field each entry of the member file names its repository by; "name" where absent. |
| `sources.<name>.kind` | string |  |
| `sources.<name>.language` | array of string | Languages, any one of which a member is written in; any where absent. |
| `sources.<name>.listed_page` | integer | How many repositories one page of the listing asks for; 100 where absent on GitHub and GitLab, and 50 on Forgejo, whose instances answer at most 50 unless configured otherwise. |
| `sources.<name>.member_hooks` | [MemberHooks](#memberhooks) | Who registers a project webhook on each member of a GitLab owner, whose group cannot send its own on GitLab Free: no one ("off"), `herald hooks <name>` ("setup"), or each poll round too ("round"); "off" where absent. Each hook points at `[server] url` and is signed with `webhook`. |

### `sources.<name>.members` of kind `gitlab`: A file beside the config, such as `repos.toml`

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>.members` | table | A file beside the config, such as `repos.toml`. |
| `sources.<name>.members.file` | string | Its path, from the config's directory. |

### `sources.<name>.members` of kind `gitlab`: A file in a repository on the source's forge, read through its API with the source's token each round

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>.members` | table | A file in a repository on the source's forge, read through its API with the source's token each round. |
| `sources.<name>.members.path` | string | The file's path in it. |
| `sources.<name>.members.ref` | string | The branch, tag or commit it is read at; the default branch where absent. |
| `sources.<name>.members.repository` | string | The repository, as `repository` writes one. |

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>.most_listed` | integer | The most repositories a listing reads before the filters; 10000 where absent. A listing over it refuses the source for the round. |
| `sources.<name>.most_members` | integer | The most members the filters may keep; 1000 where absent, and at most 100000. A selection over it refuses the source for the round. |
| `sources.<name>.most_packages` | integer | The most entries one listing of packages reads; 1000 where absent, and at most 100000. A listing over it refuses the source for the round. |
| `sources.<name>.moving` | array of string | The container tags that move from one image to the next, which are never announced, as globs as `include` writes them; `["latest"]` where absent. |
| `sources.<name>.owner` | string | The organization, GitLab group or user whose repositories are the source's members, each polled on its own, in place of `repository`. |
| `sources.<name>.package_types` | array of string | The types of package announced where the signals take "package", as the forge names them, such as `["container", "npm"]`; every type where absent. GitHub's are npm, maven, rubygems, docker, nuget and container. |
| `sources.<name>.package_visibility` | [PackageVisibility](#packagevisibility) | Who may see a package's version: the stricter of the package's own visibility and its repository's ("stricter"), or the package's own ("package"); "stricter" where absent. A version not everyone may see reaches only `private` routes under `[doctor] private_to_public = "refuse"`. |
| `sources.<name>.packages` | array of string | The packages announced, as globs as `include` writes them; every package where absent. |
| `sources.<name>.page` | integer | How many of the newest releases, and of the newest tags, one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB" where absent, and at most four times that. |
| `sources.<name>.prefer` | [Prefer](#prefer) | Which of a tag and its Release is announced where a repository's signals take both: the Release, a tag with none waiting for it as long as `tag_window` ("release"), or whichever comes first ("first"); "release" where absent. Holding a tag needs a store that keeps held events, which the markers store does not. |
| `sources.<name>.prerelease` | [PrereleaseRule](#prereleaserule) | How a pre-release is told: by the forge's flag or a pre-release part in the version ("either"), by the flag alone ("flag"), or by the version alone ("tag"); "either" where absent. A tag carries no flag, and neither does a GitLab release. |
| `sources.<name>.project` | string | The project a member's events are announced as, a template of `repository.name`, `repository.path`, `repository.owner`, `repository.id`, `repository.topics`, `repository.properties` and `repository.visibility`; the repository's name where absent. |
| `sources.<name>.properties.<name>` | [Values](#values) | A property's values: one, or any of several. |
| `sources.<name>.pulls` | table | Which pull requests are announced where the signals take "pull-request": what they did, whether drafts are, and their labels and authors; every pull request opened, ready for review or merged where absent. |
| `sources.<name>.pulls.actions` | array of [PullAction](#pullaction) | What a pull request did that is announced: `opened`, `ready_for_review`, `reopened`, `merged`, `closed`; `["opened", "ready_for_review", "merged"]` where absent. |
| `sources.<name>.pulls.authors` | array of string | The authors, as globs, whose pull requests are announced, such as `dependabot[bot]`; every author where empty. |
| `sources.<name>.pulls.drafts` | boolean | Whether a draft's opening is announced; false where absent, so a draft is announced once it is ready for review. |
| `sources.<name>.pulls.labels` | array of string | The labels, as globs as `include` writes them, any one of which a pull request announced carries; every pull request where empty. |
| `sources.<name>.registry` | string | The host of the container registry an image is named on, as in `registry.example:5000`: where absent, ghcr.io beside GitHub's API, `containers.<host>` beside a GitHub Enterprise Server's or a GHE.com tenant's, each image's own on GitLab, and the instance's on Forgejo. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.repositories.<name>.prerelease` | [PrereleaseRule](#prereleaserule) | How a pre-release of theirs is told, as the source's `prerelease` writes it; the source's where absent. |
| `sources.<name>.repositories.<name>.project` | string | The project their events are announced as, a template as the source's `project` is; the source's where absent. |
| `sources.<name>.repositories.<name>.routes` | array of string | The routes their events also reach, by id, whatever sources those routes name; each route's other keys still narrow what it takes. |
| `sources.<name>.repositories.<name>.routes_only` | boolean | Whether `routes` are the only routes their events reach, leaving out the routes that name the source; false where absent. |
| `sources.<name>.repositories.<name>.signals` | array of [Signal](#signal) | What they announce, as the source's `signals` writes it; the source's where absent. |
| `sources.<name>.repository` | string | The repository, as `owner/name`, or a GitLab project's full path, such as `group/subgroup/name`; or `owner` in its place. |
| `sources.<name>.runs` | table | Which runs are announced where the signals take "run": what they are read from, what they did, how they ended, and their branches, triggers and workflows; every finished run of the forge's own CI where absent. |
| `sources.<name>.runs.actions` | array of [RunAction](#runaction) | What a run did that is announced: `requested`, `in_progress`, `completed`; `["completed"]` where absent. |
| `sources.<name>.runs.branches` | array of string | The branches or tags runs are announced of, as globs as `include` writes them; every one where empty. One name with no glob in it is asked of the forge, so a poll reads only its runs. |
| `sources.<name>.runs.conclusions` | array of [Conclusion](#conclusion) | How a finished run ended for it to be announced; every conclusion where empty. |
| `sources.<name>.runs.from` | array of [RunFrom](#runfrom) | What runs are read from: "actions", the forge's own CI, and on GitHub "checks", the check suites and commit statuses other CI services report; `["actions"]` where absent. |
| `sources.<name>.runs.triggers` | array of string | What started the runs announced, as the forge names it, such as `push` or `schedule`; GitLab's `merge_request_event` is written `pull_request`. Every trigger where empty. |
| `sources.<name>.runs.workflows` | array of string | The workflows, pipelines, check suites' apps or status contexts whose runs are announced, as globs as `include` writes them; every one where empty. |
| `sources.<name>.signals` | array of [Signal](#signal) | What the source announces of each repository: its published Releases, its version tags, the versions of its packages, or several; `["release"]` where absent. A tag and a Release of it share one identity, so each route announces it once. |
| `sources.<name>.subgroups` | boolean | Whether a GitLab group's members include the projects of its subgroups; true where absent. |
| `sources.<name>.tables` | array of string | The arrays of the member file that list repositories, such as `["repo", "ungoverned"]`; the file's own array where absent, which only JSON can hold. |
| `sources.<name>.tag_window` | [Duration](#duration) | How long a tag with no Release waits for one, counted from when herald first sees the tag; "1h" where absent, and at most "7d". |
| `sources.<name>.template` | boolean | Whether template repositories are taken. |
| `sources.<name>.token` | table | A token that reaches a private repository or raises the API's rate limit, such as the run's `GITHUB_TOKEN`. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.topics` | array of string | Topics, any one of which a member holds; any where absent. |
| `sources.<name>.transport` | [Transport](#transport) | How an owner's members are polled; "auto" where absent. GitHub offers auto, graphql, rest and events; GitLab auto, graphql and rest; Forgejo auto, rest and feed. |
| `sources.<name>.visibility` | array of [RepositoryVisibility](#repositoryvisibility) | The visibilities taken; only public repositories where absent. A repository whose visibility the forge does not name is private. A route that takes a private or internal member is refused unless it is `private` (`[doctor] private_to_public`). |
| `sources.<name>.webhook` | table | The secret the forge signs its webhook deliveries to `herald server` with: GitHub's and Forgejo's webhook secret, or GitLab's signing token (ADR-0007). An owner source takes its repositories' and its organization's deliveries, and announces one only where the owner's last membership record holds the repository when its work runs. |
| `sources.<name>.webhook.secret` | string | The secret's name. |

### `sources.<name>` of kind `forgejo`

A Forgejo or Gitea repository's releases, tags and packages.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A Forgejo or Gitea repository's releases, tags and packages. |
| `sources.<name>.alerts` | table | Which security alerts are announced where the signals take "alert": what raised them, what they did and how severe they are; every tool's raised and reopened alerts where absent. |
| `sources.<name>.alerts.actions` | array of [AlertAction](#alertaction) | What an alert did that is announced: `created`, `reopened`, `confirmed` (GitLab), `fixed`, `dismissed`; `["created", "reopened"]` where absent. |
| `sources.<name>.alerts.severities` | array of [Severity](#severity) | How severe an alert is for it to be announced; every alert, those with no severity too, where empty. |
| `sources.<name>.alerts.tools` | array of [Tool](#tool) | What raised the alerts announced; every tool the forge raises alerts with where absent. |
| `sources.<name>.api` | string | The API's base URL, for a forge that is not GitHub, GitLab.com or Codeberg: a GitHub Enterprise Server's `https://ghe.example/api/v3`, a GHE.com tenant's `https://api.acme.ghe.com`, a self-managed or Dedicated GitLab's `https://gitlab.example/api/v4`, or a Forgejo's `https://forge.example/api/v1`. Where absent, it is the CI run's own when announcing the run's repository, and otherwise the forge's public instance. An owner is listed at the same base. |
| `sources.<name>.app` | string | The GitHub App the source reads as, by the name of its `[github_apps.<name>]` table, in place of `token`: herald mints an installation token for each round. |
| `sources.<name>.archived` | boolean | Whether archived repositories are taken. |
| `sources.<name>.backfill` | integer | How many of the newest releases, tags and package versions the first poll announces; none where absent (ADR-0003). For an owner, it applies to the members the first listing finds; a member found later announces only what it publishes after it was found. |
| `sources.<name>.discovery` | [Discovery](#discovery) | How an owner source finds the repositories that join or leave it: listing the owner each round ("list"), or only where it holds no listing, its selection changed or a delivery said the owner's repositories changed ("push"); "list" where absent. |
| `sources.<name>.edits_depth` | integer | How many of the newest Releases a poll watches for an edit of their name or notes, where a route taking the source edits them (`edits = "release"`); 30 where absent, and at most `page`. |
| `sources.<name>.exclude` | array of string | The names left out, as `include` writes them, whatever `include` says. |
| `sources.<name>.feed_page` | integer | How many activities one page of a Forgejo owner's feed asks for under `transport = "feed"`; 50 where absent, and at most 200. |
| `sources.<name>.feed_pages` | integer | How many pages of a Forgejo owner's feed a round reads back before it polls every member; 3 where absent, and at most 12. |
| `sources.<name>.fork` | boolean | Whether forks are taken. |
| `sources.<name>.graphql` | string | The GraphQL endpoint of a GitHub or GitLab owner source; beside `api` where absent: `/api/graphql` on a GitHub Enterprise Server whose `api` ends in `/api/v3` and on GitLab, whose `api` ends in `/api/v4`, and `/graphql` on GitHub and GHE.com. |
| `sources.<name>.graphql_batch` | integer | How many members one GraphQL request asks for; 50 where absent, and at most 100. |
| `sources.<name>.include` | array of string | The names of the owner's repositories taken, as globs in which `*` stands for any run of characters, `?` for any one and `[...]` for any one of a class, ignoring case; every name where absent. |
| `sources.<name>.installation` | integer | The id of the App's installation tokens are minted for; looked up on the owner, or on the repository, where absent. |
| `sources.<name>.issues` | table | Which issues are announced where the signals take "issue": what they did, and their labels and authors; every issue opened, closed or reopened where absent. |
| `sources.<name>.issues.actions` | array of [IssueAction](#issueaction) | What an issue did that is announced: `opened`, `closed`, `reopened`; every one where absent. |
| `sources.<name>.issues.authors` | array of string | The authors, as globs, whose issues are announced; every author where empty. |
| `sources.<name>.issues.confidential` | boolean | Whether GitLab's confidential issues are announced, each seen as private, so only a route declared private takes it; false where absent. |
| `sources.<name>.issues.labels` | array of string | The labels, as globs as `include` writes them, any one of which an issue announced carries; every issue where empty. |
| `sources.<name>.keep_members` | [Duration](#duration) | How long a member that left the selection keeps its cursor before it is forgotten; "30d" where absent. |
| `sources.<name>.key` | string | The field each entry of the member file names its repository by; "name" where absent. |
| `sources.<name>.kind` | string |  |
| `sources.<name>.language` | array of string | Languages, any one of which a member is written in; any where absent. |
| `sources.<name>.listed_page` | integer | How many repositories one page of the listing asks for; 100 where absent on GitHub and GitLab, and 50 on Forgejo, whose instances answer at most 50 unless configured otherwise. |
| `sources.<name>.member_hooks` | [MemberHooks](#memberhooks) | Who registers a project webhook on each member of a GitLab owner, whose group cannot send its own on GitLab Free: no one ("off"), `herald hooks <name>` ("setup"), or each poll round too ("round"); "off" where absent. Each hook points at `[server] url` and is signed with `webhook`. |

### `sources.<name>.members` of kind `forgejo`: A file beside the config, such as `repos.toml`

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>.members` | table | A file beside the config, such as `repos.toml`. |
| `sources.<name>.members.file` | string | Its path, from the config's directory. |

### `sources.<name>.members` of kind `forgejo`: A file in a repository on the source's forge, read through its API with the source's token each round

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>.members` | table | A file in a repository on the source's forge, read through its API with the source's token each round. |
| `sources.<name>.members.path` | string | The file's path in it. |
| `sources.<name>.members.ref` | string | The branch, tag or commit it is read at; the default branch where absent. |
| `sources.<name>.members.repository` | string | The repository, as `repository` writes one. |

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>.most_listed` | integer | The most repositories a listing reads before the filters; 10000 where absent. A listing over it refuses the source for the round. |
| `sources.<name>.most_members` | integer | The most members the filters may keep; 1000 where absent, and at most 100000. A selection over it refuses the source for the round. |
| `sources.<name>.most_packages` | integer | The most entries one listing of packages reads; 1000 where absent, and at most 100000. A listing over it refuses the source for the round. |
| `sources.<name>.moving` | array of string | The container tags that move from one image to the next, which are never announced, as globs as `include` writes them; `["latest"]` where absent. |
| `sources.<name>.owner` | string | The organization, GitLab group or user whose repositories are the source's members, each polled on its own, in place of `repository`. |
| `sources.<name>.package_types` | array of string | The types of package announced where the signals take "package", as the forge names them, such as `["container", "npm"]`; every type where absent. GitHub's are npm, maven, rubygems, docker, nuget and container. |
| `sources.<name>.package_visibility` | [PackageVisibility](#packagevisibility) | Who may see a package's version: the stricter of the package's own visibility and its repository's ("stricter"), or the package's own ("package"); "stricter" where absent. A version not everyone may see reaches only `private` routes under `[doctor] private_to_public = "refuse"`. |
| `sources.<name>.packages` | array of string | The packages announced, as globs as `include` writes them; every package where absent. |
| `sources.<name>.page` | integer | How many of the newest releases, and of the newest tags, one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB" where absent, and at most four times that. |
| `sources.<name>.prefer` | [Prefer](#prefer) | Which of a tag and its Release is announced where a repository's signals take both: the Release, a tag with none waiting for it as long as `tag_window` ("release"), or whichever comes first ("first"); "release" where absent. Holding a tag needs a store that keeps held events, which the markers store does not. |
| `sources.<name>.prerelease` | [PrereleaseRule](#prereleaserule) | How a pre-release is told: by the forge's flag or a pre-release part in the version ("either"), by the flag alone ("flag"), or by the version alone ("tag"); "either" where absent. A tag carries no flag, and neither does a GitLab release. |
| `sources.<name>.project` | string | The project a member's events are announced as, a template of `repository.name`, `repository.path`, `repository.owner`, `repository.id`, `repository.topics`, `repository.properties` and `repository.visibility`; the repository's name where absent. |
| `sources.<name>.properties.<name>` | [Values](#values) | A property's values: one, or any of several. |
| `sources.<name>.pulls` | table | Which pull requests are announced where the signals take "pull-request": what they did, whether drafts are, and their labels and authors; every pull request opened, ready for review or merged where absent. |
| `sources.<name>.pulls.actions` | array of [PullAction](#pullaction) | What a pull request did that is announced: `opened`, `ready_for_review`, `reopened`, `merged`, `closed`; `["opened", "ready_for_review", "merged"]` where absent. |
| `sources.<name>.pulls.authors` | array of string | The authors, as globs, whose pull requests are announced, such as `dependabot[bot]`; every author where empty. |
| `sources.<name>.pulls.drafts` | boolean | Whether a draft's opening is announced; false where absent, so a draft is announced once it is ready for review. |
| `sources.<name>.pulls.labels` | array of string | The labels, as globs as `include` writes them, any one of which a pull request announced carries; every pull request where empty. |
| `sources.<name>.registry` | string | The host of the container registry an image is named on, as in `registry.example:5000`: where absent, ghcr.io beside GitHub's API, `containers.<host>` beside a GitHub Enterprise Server's or a GHE.com tenant's, each image's own on GitLab, and the instance's on Forgejo. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.repositories.<name>.prerelease` | [PrereleaseRule](#prereleaserule) | How a pre-release of theirs is told, as the source's `prerelease` writes it; the source's where absent. |
| `sources.<name>.repositories.<name>.project` | string | The project their events are announced as, a template as the source's `project` is; the source's where absent. |
| `sources.<name>.repositories.<name>.routes` | array of string | The routes their events also reach, by id, whatever sources those routes name; each route's other keys still narrow what it takes. |
| `sources.<name>.repositories.<name>.routes_only` | boolean | Whether `routes` are the only routes their events reach, leaving out the routes that name the source; false where absent. |
| `sources.<name>.repositories.<name>.signals` | array of [Signal](#signal) | What they announce, as the source's `signals` writes it; the source's where absent. |
| `sources.<name>.repository` | string | The repository, as `owner/name`, or a GitLab project's full path, such as `group/subgroup/name`; or `owner` in its place. |
| `sources.<name>.runs` | table | Which runs are announced where the signals take "run": what they are read from, what they did, how they ended, and their branches, triggers and workflows; every finished run of the forge's own CI where absent. |
| `sources.<name>.runs.actions` | array of [RunAction](#runaction) | What a run did that is announced: `requested`, `in_progress`, `completed`; `["completed"]` where absent. |
| `sources.<name>.runs.branches` | array of string | The branches or tags runs are announced of, as globs as `include` writes them; every one where empty. One name with no glob in it is asked of the forge, so a poll reads only its runs. |
| `sources.<name>.runs.conclusions` | array of [Conclusion](#conclusion) | How a finished run ended for it to be announced; every conclusion where empty. |
| `sources.<name>.runs.from` | array of [RunFrom](#runfrom) | What runs are read from: "actions", the forge's own CI, and on GitHub "checks", the check suites and commit statuses other CI services report; `["actions"]` where absent. |
| `sources.<name>.runs.triggers` | array of string | What started the runs announced, as the forge names it, such as `push` or `schedule`; GitLab's `merge_request_event` is written `pull_request`. Every trigger where empty. |
| `sources.<name>.runs.workflows` | array of string | The workflows, pipelines, check suites' apps or status contexts whose runs are announced, as globs as `include` writes them; every one where empty. |
| `sources.<name>.signals` | array of [Signal](#signal) | What the source announces of each repository: its published Releases, its version tags, the versions of its packages, or several; `["release"]` where absent. A tag and a Release of it share one identity, so each route announces it once. |
| `sources.<name>.subgroups` | boolean | Whether a GitLab group's members include the projects of its subgroups; true where absent. |
| `sources.<name>.tables` | array of string | The arrays of the member file that list repositories, such as `["repo", "ungoverned"]`; the file's own array where absent, which only JSON can hold. |
| `sources.<name>.tag_window` | [Duration](#duration) | How long a tag with no Release waits for one, counted from when herald first sees the tag; "1h" where absent, and at most "7d". |
| `sources.<name>.template` | boolean | Whether template repositories are taken. |
| `sources.<name>.token` | table | A token that reaches a private repository or raises the API's rate limit, such as the run's `GITHUB_TOKEN`. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.topics` | array of string | Topics, any one of which a member holds; any where absent. |
| `sources.<name>.transport` | [Transport](#transport) | How an owner's members are polled; "auto" where absent. GitHub offers auto, graphql, rest and events; GitLab auto, graphql and rest; Forgejo auto, rest and feed. |
| `sources.<name>.visibility` | array of [RepositoryVisibility](#repositoryvisibility) | The visibilities taken; only public repositories where absent. A repository whose visibility the forge does not name is private. A route that takes a private or internal member is refused unless it is `private` (`[doctor] private_to_public`). |
| `sources.<name>.webhook` | table | The secret the forge signs its webhook deliveries to `herald server` with: GitHub's and Forgejo's webhook secret, or GitLab's signing token (ADR-0007). An owner source takes its repositories' and its organization's deliveries, and announces one only where the owner's last membership record holds the repository when its work runs. |
| `sources.<name>.webhook.secret` | string | The secret's name. |

### `sources.<name>` of kind `ghcr`

A GitHub owner's container images, each announced as the project of the repository it is linked to.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A GitHub owner's container images, each announced as the project of the repository it is linked to. |
| `sources.<name>.api` | string | GitHub's REST API, for a GitHub Enterprise Server or a GHE.com tenant; `https://api.github.com` where absent. |
| `sources.<name>.backfill` | integer | How many of the newest versions the first poll announces; none where absent. |
| `sources.<name>.exclude` | array of string | Globs of the image names left out, whatever `include` says. |
| `sources.<name>.include` | array of string | Globs of the image names taken; every name where empty. |
| `sources.<name>.keep_members` | [Duration](#duration) | How long an image that left the owner is kept, with its cursor; "30d" where absent. |
| `sources.<name>.kind` | string |  |
| `sources.<name>.most_packages` | integer | The most images a listing reads before it refuses the round; 1000 where absent, and at most 100000. |
| `sources.<name>.moving` | array of string | Globs of the tags never announced, as they move from one image to the next; `["latest"]` where absent. |
| `sources.<name>.owner` | string | The GitHub organization or user whose container images the source takes. |
| `sources.<name>.package_visibility` | [PackageVisibility](#packagevisibility) | Who may see an image linked to a repository: `stricter`, the stricter of the image's visibility and its repository's, where absent, or `package`, the image's own. |
| `sources.<name>.page` | integer | How many of an image's newest versions one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB" where absent, and at most four times that. |
| `sources.<name>.project` | string | The project an image linked to a repository is announced as: a template over the repository's `repository.*`, as an owner source renders it; the repository's name where absent. An image linked to none is announced as its own name. |
| `sources.<name>.registry` | string | The registry's host the images are named on, in their identities; ghcr.io beside api.github.com, and `containers.<host>` beside a GitHub Enterprise Server or a GHE.com tenant, where absent. |
| `sources.<name>.remembered` | integer | How many identities each image's cursor remembers; 500 where absent, and at least `page`. |
| `sources.<name>.token` | table | A token that may read the owner's packages, which GitHub asks for even for public images. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.visibility` | array of [RepositoryVisibility](#repositoryvisibility) | The visibilities of the images taken; `["public"]` where absent. |

### `sources.<name>` of kind `packagist`

A package's versions on Packagist.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A package's versions on Packagist. |
| `sources.<name>.api` | string | The registry's base URL, for a mirror or a private registry that answers as the public one does; the public registry's where absent. |
| `sources.<name>.backfill` | integer | How many of the newest versions the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.exclude` | array of string | Globs of the package names a namespace source leaves out, whatever `include` says. |
| `sources.<name>.include` | array of string | Globs of the package names a namespace source takes; every name where empty. |
| `sources.<name>.keep_members` | [Duration](#duration) | How long a package that left the namespace is kept, with its cursor, before it is forgotten; "30d" where absent. |
| `sources.<name>.kind` | string |  |
| `sources.<name>.listed_page` | integer | How many packages one page of a namespace's listing asks for, where the registry pages it; the most it allows where absent. |
| `sources.<name>.listing` | [VendorListing](#vendorlisting) | How a Packagist vendor's packages are listed: `packagist`, through `/packages/list.json` at `listing_api`, where absent, or `composer`, through a Composer repository's `packages.json` at `api`. |
| `sources.<name>.listing_api` | string | The base URL serving Packagist's `/packages/list.json` for a vendor source; `https://packagist.org` where absent. |
| `sources.<name>.most_listed` | integer | The most packages a namespace's listing reads before `include` and `exclude`; 10000 where absent. A namespace listing more is refused for the round. |
| `sources.<name>.most_members` | integer | The most packages `include` and `exclude` may keep; 1000 where absent, and at most 100000. A namespace keeping more is refused for the round. |
| `sources.<name>.namespace` | string | The Docker Hub namespace whose repositories the source takes, each polled on its own; exclusive with `package`. |
| `sources.<name>.ordering` | [TagOrdering](#tagordering) | How Docker Hub lists a repository's tags for a poll: `last_updated`, the most recently pushed first, where unset, or `name`. |
| `sources.<name>.owner` | string | The crates.io owner whose crates the source takes, each polled on its own, as crates.io writes owners: `user:<login>` or `team:github:<org>:<team>`; exclusive with `package`. |
| `sources.<name>.package` | string | The package, as the registry names it: `vendor/name` on Packagist, a crate's name, an npm package's name such as `@scope/name`, or a Docker Hub repository such as `library/caddy`, where an official image may leave out `library/`. |
| `sources.<name>.page` | integer | How many of the newest versions one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB", "16MiB" on Packagist and "32MiB" on npm, where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.scope` | string | The npm scope whose packages the source takes, each polled on its own, such as `@acme`; exclusive with `package`. |
| `sources.<name>.token` | table | A token for a private package: npm's, sent as a bearer token; Private Packagist's Composer token, sent as HTTP Basic with the user `token`; or a Docker Hub personal access token, exchanged with `username` for a bearer token. crates.io takes none. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.unscoped` | boolean | Whether an npm scope source also takes the packages its organization owns outside the scope. |
| `sources.<name>.username` | string | The Docker Hub account `token` belongs to. |
| `sources.<name>.vendor` | string | The Packagist vendor whose packages the source takes, each polled on its own; exclusive with `package`. |
| `sources.<name>.visibility` | [Declared](#declared) | Who may see a namespace's packages where its listing does not say: `public` declares them public. Where absent, they are public when the source has no token and private when it has one, which a token may list. |

### `sources.<name>` of kind `crates-io`

A crate's versions on crates.io.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A crate's versions on crates.io. |
| `sources.<name>.api` | string | The registry's base URL, for a mirror or a private registry that answers as the public one does; the public registry's where absent. |
| `sources.<name>.backfill` | integer | How many of the newest versions the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.exclude` | array of string | Globs of the package names a namespace source leaves out, whatever `include` says. |
| `sources.<name>.include` | array of string | Globs of the package names a namespace source takes; every name where empty. |
| `sources.<name>.keep_members` | [Duration](#duration) | How long a package that left the namespace is kept, with its cursor, before it is forgotten; "30d" where absent. |
| `sources.<name>.kind` | string |  |
| `sources.<name>.listed_page` | integer | How many packages one page of a namespace's listing asks for, where the registry pages it; the most it allows where absent. |
| `sources.<name>.listing` | [VendorListing](#vendorlisting) | How a Packagist vendor's packages are listed: `packagist`, through `/packages/list.json` at `listing_api`, where absent, or `composer`, through a Composer repository's `packages.json` at `api`. |
| `sources.<name>.listing_api` | string | The base URL serving Packagist's `/packages/list.json` for a vendor source; `https://packagist.org` where absent. |
| `sources.<name>.most_listed` | integer | The most packages a namespace's listing reads before `include` and `exclude`; 10000 where absent. A namespace listing more is refused for the round. |
| `sources.<name>.most_members` | integer | The most packages `include` and `exclude` may keep; 1000 where absent, and at most 100000. A namespace keeping more is refused for the round. |
| `sources.<name>.namespace` | string | The Docker Hub namespace whose repositories the source takes, each polled on its own; exclusive with `package`. |
| `sources.<name>.ordering` | [TagOrdering](#tagordering) | How Docker Hub lists a repository's tags for a poll: `last_updated`, the most recently pushed first, where unset, or `name`. |
| `sources.<name>.owner` | string | The crates.io owner whose crates the source takes, each polled on its own, as crates.io writes owners: `user:<login>` or `team:github:<org>:<team>`; exclusive with `package`. |
| `sources.<name>.package` | string | The package, as the registry names it: `vendor/name` on Packagist, a crate's name, an npm package's name such as `@scope/name`, or a Docker Hub repository such as `library/caddy`, where an official image may leave out `library/`. |
| `sources.<name>.page` | integer | How many of the newest versions one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB", "16MiB" on Packagist and "32MiB" on npm, where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.scope` | string | The npm scope whose packages the source takes, each polled on its own, such as `@acme`; exclusive with `package`. |
| `sources.<name>.token` | table | A token for a private package: npm's, sent as a bearer token; Private Packagist's Composer token, sent as HTTP Basic with the user `token`; or a Docker Hub personal access token, exchanged with `username` for a bearer token. crates.io takes none. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.unscoped` | boolean | Whether an npm scope source also takes the packages its organization owns outside the scope. |
| `sources.<name>.username` | string | The Docker Hub account `token` belongs to. |
| `sources.<name>.vendor` | string | The Packagist vendor whose packages the source takes, each polled on its own; exclusive with `package`. |
| `sources.<name>.visibility` | [Declared](#declared) | Who may see a namespace's packages where its listing does not say: `public` declares them public. Where absent, they are public when the source has no token and private when it has one, which a token may list. |

### `sources.<name>` of kind `npm`

A package's versions on the npm registry.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A package's versions on the npm registry. |
| `sources.<name>.api` | string | The registry's base URL, for a mirror or a private registry that answers as the public one does; the public registry's where absent. |
| `sources.<name>.backfill` | integer | How many of the newest versions the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.exclude` | array of string | Globs of the package names a namespace source leaves out, whatever `include` says. |
| `sources.<name>.include` | array of string | Globs of the package names a namespace source takes; every name where empty. |
| `sources.<name>.keep_members` | [Duration](#duration) | How long a package that left the namespace is kept, with its cursor, before it is forgotten; "30d" where absent. |
| `sources.<name>.kind` | string |  |
| `sources.<name>.listed_page` | integer | How many packages one page of a namespace's listing asks for, where the registry pages it; the most it allows where absent. |
| `sources.<name>.listing` | [VendorListing](#vendorlisting) | How a Packagist vendor's packages are listed: `packagist`, through `/packages/list.json` at `listing_api`, where absent, or `composer`, through a Composer repository's `packages.json` at `api`. |
| `sources.<name>.listing_api` | string | The base URL serving Packagist's `/packages/list.json` for a vendor source; `https://packagist.org` where absent. |
| `sources.<name>.most_listed` | integer | The most packages a namespace's listing reads before `include` and `exclude`; 10000 where absent. A namespace listing more is refused for the round. |
| `sources.<name>.most_members` | integer | The most packages `include` and `exclude` may keep; 1000 where absent, and at most 100000. A namespace keeping more is refused for the round. |
| `sources.<name>.namespace` | string | The Docker Hub namespace whose repositories the source takes, each polled on its own; exclusive with `package`. |
| `sources.<name>.ordering` | [TagOrdering](#tagordering) | How Docker Hub lists a repository's tags for a poll: `last_updated`, the most recently pushed first, where unset, or `name`. |
| `sources.<name>.owner` | string | The crates.io owner whose crates the source takes, each polled on its own, as crates.io writes owners: `user:<login>` or `team:github:<org>:<team>`; exclusive with `package`. |
| `sources.<name>.package` | string | The package, as the registry names it: `vendor/name` on Packagist, a crate's name, an npm package's name such as `@scope/name`, or a Docker Hub repository such as `library/caddy`, where an official image may leave out `library/`. |
| `sources.<name>.page` | integer | How many of the newest versions one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB", "16MiB" on Packagist and "32MiB" on npm, where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.scope` | string | The npm scope whose packages the source takes, each polled on its own, such as `@acme`; exclusive with `package`. |
| `sources.<name>.token` | table | A token for a private package: npm's, sent as a bearer token; Private Packagist's Composer token, sent as HTTP Basic with the user `token`; or a Docker Hub personal access token, exchanged with `username` for a bearer token. crates.io takes none. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.unscoped` | boolean | Whether an npm scope source also takes the packages its organization owns outside the scope. |
| `sources.<name>.username` | string | The Docker Hub account `token` belongs to. |
| `sources.<name>.vendor` | string | The Packagist vendor whose packages the source takes, each polled on its own; exclusive with `package`. |
| `sources.<name>.visibility` | [Declared](#declared) | Who may see a namespace's packages where its listing does not say: `public` declares them public. Where absent, they are public when the source has no token and private when it has one, which a token may list. |

### `sources.<name>` of kind `docker-hub`

A repository's tags on Docker Hub.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A repository's tags on Docker Hub. |
| `sources.<name>.api` | string | The registry's base URL, for a mirror or a private registry that answers as the public one does; the public registry's where absent. |
| `sources.<name>.backfill` | integer | How many of the newest versions the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.exclude` | array of string | Globs of the package names a namespace source leaves out, whatever `include` says. |
| `sources.<name>.include` | array of string | Globs of the package names a namespace source takes; every name where empty. |
| `sources.<name>.keep_members` | [Duration](#duration) | How long a package that left the namespace is kept, with its cursor, before it is forgotten; "30d" where absent. |
| `sources.<name>.kind` | string |  |
| `sources.<name>.listed_page` | integer | How many packages one page of a namespace's listing asks for, where the registry pages it; the most it allows where absent. |
| `sources.<name>.listing` | [VendorListing](#vendorlisting) | How a Packagist vendor's packages are listed: `packagist`, through `/packages/list.json` at `listing_api`, where absent, or `composer`, through a Composer repository's `packages.json` at `api`. |
| `sources.<name>.listing_api` | string | The base URL serving Packagist's `/packages/list.json` for a vendor source; `https://packagist.org` where absent. |
| `sources.<name>.most_listed` | integer | The most packages a namespace's listing reads before `include` and `exclude`; 10000 where absent. A namespace listing more is refused for the round. |
| `sources.<name>.most_members` | integer | The most packages `include` and `exclude` may keep; 1000 where absent, and at most 100000. A namespace keeping more is refused for the round. |
| `sources.<name>.namespace` | string | The Docker Hub namespace whose repositories the source takes, each polled on its own; exclusive with `package`. |
| `sources.<name>.ordering` | [TagOrdering](#tagordering) | How Docker Hub lists a repository's tags for a poll: `last_updated`, the most recently pushed first, where unset, or `name`. |
| `sources.<name>.owner` | string | The crates.io owner whose crates the source takes, each polled on its own, as crates.io writes owners: `user:<login>` or `team:github:<org>:<team>`; exclusive with `package`. |
| `sources.<name>.package` | string | The package, as the registry names it: `vendor/name` on Packagist, a crate's name, an npm package's name such as `@scope/name`, or a Docker Hub repository such as `library/caddy`, where an official image may leave out `library/`. |
| `sources.<name>.page` | integer | How many of the newest versions one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB", "16MiB" on Packagist and "32MiB" on npm, where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.scope` | string | The npm scope whose packages the source takes, each polled on its own, such as `@acme`; exclusive with `package`. |
| `sources.<name>.token` | table | A token for a private package: npm's, sent as a bearer token; Private Packagist's Composer token, sent as HTTP Basic with the user `token`; or a Docker Hub personal access token, exchanged with `username` for a bearer token. crates.io takes none. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.unscoped` | boolean | Whether an npm scope source also takes the packages its organization owns outside the scope. |
| `sources.<name>.username` | string | The Docker Hub account `token` belongs to. |
| `sources.<name>.vendor` | string | The Packagist vendor whose packages the source takes, each polled on its own; exclusive with `package`. |
| `sources.<name>.visibility` | [Declared](#declared) | Who may see a namespace's packages where its listing does not say: `public` declares them public. Where absent, they are public when the source has no token and private when it has one, which a token may list. |

### `sources.<name>` of kind `oci`

A container image's tags on any registry serving the OCI Distribution API.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A container image's tags on any registry serving the OCI Distribution API. |
| `sources.<name>.auth` | string | The base URL of the one origin besides the registry's own that may hand out its tokens: a `Bearer` challenge naming a realm anywhere else is refused. `https://auth.docker.io` for Docker Hub's registry where absent, and none otherwise; GitLab's registry needs its GitLab's, such as `https://gitlab.com`. |
| `sources.<name>.backfill` | integer | How many of the newest tags the first poll announces; none where absent. The registry gives tags no time, so the first poll reads the last `page` tags its listing names to find the newest. |
| `sources.<name>.blobs` | array of string | The base URLs of the origins a config blob may be redirected to, fetched without the registry's credential. Docker Hub's and GitHub's CDNs for their registries where absent, and none otherwise. A blob redirected elsewhere is not read: its tag takes the time it was first seen. |
| `sources.<name>.catalog` | boolean | Whether the source takes every repository the registry's `/v2/_catalog` lists, each polled on its own. Exclusive with `repository`. |
| `sources.<name>.exclude` | array of string | Globs of the repositories a catalog source leaves out, whatever `include` says. |
| `sources.<name>.include` | array of string | Globs of the repositories a catalog source takes; every one where empty. |
| `sources.<name>.keep_members` | [Duration](#duration) | How long a repository that left the catalog is kept, with its cursor, before it is forgotten; "30d" where absent. |
| `sources.<name>.kind` | string |  |
| `sources.<name>.listed_page` | integer | How many tags, or repositories of the catalog, one page of a listing asks for; 1000 where absent, and at most 10000. |
| `sources.<name>.most_listed` | integer | The most repositories the catalog reads before `include` and `exclude`; 10000 where absent. A catalog listing more is refused for the round. |
| `sources.<name>.most_members` | integer | The most repositories `include` and `exclude` may keep; 1000 where absent, and at most 100000. A catalog keeping more is refused for the round. |
| `sources.<name>.most_tags` | integer | The most tags a repository's listing reads; at most 100000. A repository listing more is refused for the round. The cursor remembers every tag, so where absent it is 10000, or as many as the store keeps in one record where that is fewer, which the doctor names; a value the store's records cannot hold is refused. |
| `sources.<name>.moving` | array of string | Globs of the tags that move, such as `latest`, which are never announced; `["latest"]` where absent. |
| `sources.<name>.page` | integer | How many unseen tags one poll reads; 30 where absent. Each takes up to four requests. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB" where absent, and at most four times that. |
| `sources.<name>.registry` | string | The registry's base URL, which serves the OCI Distribution API under `/v2/`, such as `https://registry-1.docker.io`, `https://ghcr.io` or `https://registry.gitlab.com`. |
| `sources.<name>.registry_name` | string | The registry's name in each tag's identity, such as `docker.io` for a mirror of Docker Hub, so the tag is announced once with any other source of the image; `docker.io` for Docker Hub's registry where absent, and the registry's host otherwise. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers; `most_tags` where absent, and at least that. |
| `sources.<name>.repository` | string | The repository whose tags the source takes, such as `acme/app`; on Docker Hub an official image may leave out `library/`. Exclusive with `catalog`. |
| `sources.<name>.token` | table | A token or password for a private repository, sent with `username`. Anonymous where absent. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.username` | string | The account `token` belongs to, sent with it as HTTP Basic to the registry's token service, or to the registry where it asks for Basic. |
| `sources.<name>.visibility` | [Declared](#declared) | Who may see the repositories: `public` declares them public. Where absent, they are public when the source has no token and private when it has one. |

### `sources.<name>` of kind `feed`

An RSS or Atom feed's entries.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | An RSS or Atom feed's entries. |
| `sources.<name>.backfill` | integer | How many of the newest entries the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.kind` | string |  |
| `sources.<name>.page` | integer | How many of the newest entries one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB" where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.url` | string | The feed's URL, RSS 2.0 or Atom 1.0. |

### `sources.<name>` of kind `herald`

Another herald's feed, each answer and envelope checked against the keys trusted for it.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | Another herald's feed, each answer and envelope checked against the keys trusted for it. |
| `sources.<name>.backfill` | integer | How many of the newest envelopes the first pull announces; none where absent, and at most `pages` pages of 80. |
| `sources.<name>.feed` | string | The name of the peer's feed, as its discovery document lists it. |
| `sources.<name>.keys` | array of string | The Ed25519 keys pinned for the instance `url` names, each a Multikey its operator gave out of band. |
| `sources.<name>.kind` | string |  |
| `sources.<name>.namespaces` | array of string | The identities the peer may give as they are, each the start of identities up to a '/', such as "github.com/123456"; every other is taken as herald/<peer>/<identity>. None where absent. |
| `sources.<name>.pages` | integer | How many pages one pull reads at most: 5 where absent, and at most 20. |
| `sources.<name>.peer` | string | The peer of `[peers]` whose feed it pulls, through the keys pinned for it; with `feed`, in place of `url`. |
| `sources.<name>.peer_share` | integer | How many tombstones of identities it took, which it never forgets, it keeps of any one instance before it takes no new identity of it: 1000 where absent, and at most 4000. A pull reads such an envelope again later; a tombstone of an identity it took is always taken. |
| `sources.<name>.peer_tombstones` | integer | How many tombstones of identities it never took it keeps of any one instance: 250 where absent, or `tombstones` where that is fewer, and at most `tombstones`. |
| `sources.<name>.tombstones` | integer | How many tombstones of identities it never took it keeps the proof of in all, so no older envelope of them is taken again, apart from those of identities it took, which it never forgets: 1000 where absent, and at most 4000. It forgets those taken first. |
| `sources.<name>.url` | string | The full URL of a feed of an instance that is no peer, whose keys `keys` pins; in place of `peer` and `feed`. |
| `sources.<name>.verify` | [Verify](#verify) | Which envelopes it takes: "origin" (where absent), those the peer proved itself, or "hop", also those it passes on from another peer. |

## `store`

The store herald keeps its records in.

### `store` of kind `sqlite`

A SQLite file, on a volume or a persistent disk.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.busy` | [Duration](#duration) | How long a transaction waits for another process to let go of the file; 5s where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.key` | table | The secret holding a key to seal every record with, such as `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt (ADR-0005); records are not sealed where absent. |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.path` | string | The database file, relative to `herald.toml`; `herald.sqlite` beside it where absent. |

### `store` of kind `postgres`

A Postgres database.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.connect_timeout` | [Duration](#duration) | How long making a connection may take; 10s where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.key` | table | The secret holding a key to seal every record with, such as `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt (ADR-0005); records are not sealed where absent. |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.statement_timeout` | [Duration](#duration) | How long one statement may run before the database cancels it; 30s where absent. |
| `store.url` | table | The secret holding the database's connection URL, such as `postgres://herald:…@db.example/herald`. herald reaches it over TLS unless the URL sets `sslmode=disable`. |
| `store.url.secret` | string | The secret's name. |

### `store` of kind `d1`

Cloudflare D1, through the Worker's binding.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.binding` | string | The Worker's binding to the database, as `wrangler.toml` names it; `HERALD_DB` where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.key` | table | The secret holding a key to seal every record with, such as `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt (ADR-0005); records are not sealed where absent. |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |

### `store` of kind `durable-object`

The storage of herald's Cloudflare Durable Object, through the Worker's binding.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.binding` | string | The Worker's binding to herald's Durable Object namespace, as `wrangler.toml` names it; `HERALD` where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.key` | table | The secret holding a key to seal every record with, such as `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt (ADR-0005); records are not sealed where absent. |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.object` | string | The name of the object that keeps the records; `herald` where absent. |

### `store` of kind `deno-kv`

Deno KV, on Deno Deploy or wherever a database is opened.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.key` | table | The secret holding a key to seal every record with, such as `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt (ADR-0005); records are not sealed where absent. |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.path` | string | The database `Deno.openKv` opens: a path or a URL; the deployment's own where absent. |
| `store.prefix` | string | The key every record is kept under, its parts split at `/`; `herald` where absent. |

### `store` of kind `fastly-kv`

A Fastly KV Store, on Fastly Compute.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.key` | table | The secret holding a key to seal every record with, such as `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt (ADR-0005); records are not sealed where absent. |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.prefix` | string | The key every record is kept under, its parts split at `/`; `herald` where absent. |
| `store.store` | string | The KV Store the service is linked to, by name; `herald` where absent. |
| `store.tombstone` | [Duration](#duration) | How long a deleted record's mark is kept before Fastly expires it, so a write that read the record before it was deleted fails; "1h" where absent. |

### `store` of kind `dynamodb`

An AWS DynamoDB table.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.access_key_id` | table | The secret holding the access key's id; `AWS_ACCESS_KEY_ID` where absent, as a Lambda's role provides it. |
| `store.access_key_id.secret` | string | The secret's name. |
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.endpoint` | string | The endpoint's base URL, for DynamoDB Local or another endpoint; the region's own where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.key` | table | The secret holding a key to seal every record with, such as `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt (ADR-0005); records are not sealed where absent. |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.region` | string | The region, such as `eu-west-1`; the `AWS_REGION` the platform sets where absent. |
| `store.secret_access_key` | table | The secret holding the secret access key; `AWS_SECRET_ACCESS_KEY` where absent. |
| `store.secret_access_key.secret` | string | The secret's name. |
| `store.session_token` | table | The secret holding a session token, sent where it is set; `AWS_SESSION_TOKEN` where absent. |
| `store.session_token.secret` | string | The secret's name. |
| `store.table` | string | The table, with the string partition key `directory` and the string sort key `name`; `herald` where absent. |

### `store` of kind `git-branch`

One file per record on a `herald-state` branch.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.api` | string | The API's base URL, for a GitHub Enterprise Server; GitHub's own API where absent. |
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.branch` | string | The branch the records are kept on; `herald-state` where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.key` | table | The secret holding the key every record is sealed with, such as `HERALD_STORE_KEY`: whoever can write the repository could otherwise forge records herald trusts (ADR-0005). |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.repository` | string | The repository, as `owner/name`; the run's own where absent. |
| `store.token` | table | The secret holding a token that may push to the branch; `GITHUB_TOKEN` where absent. |
| `store.token.secret` | string | The secret's name. |

### `store` of kind `markers`

The destinations' own posts.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.kind` | string |  |

### `store` of kind `actions-cache`

Generations of the GitHub Actions cache.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keeps` | array of [RecordKind](#recordkind) | What it keeps besides the ledger, cursors, schedules and deliveries: any of "subscriptions", "pins", "notes", "latest" and "keys". Each is rewritten with every other record on every write, and counts against the cache's quota; none where absent. |
| `store.key` | table | The secret holding the key every record is sealed with, such as `HERALD_STORE_KEY`: whoever can write the repository could otherwise forge records herald trusts (ADR-0005). |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.prefix` | string | What every generation's key starts with, so two configs in one repository keep their records apart; `herald-state` where absent. |

### `store` of kind `repo-variables`

The repository's variables.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.api` | string | The API's base URL, for a GitHub Enterprise Server; GitHub's own API where absent. |
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.key` | table | The secret holding the key every record is sealed with, such as `HERALD_STORE_KEY`: whoever can write the repository could otherwise forge records herald trusts (ADR-0005). |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.lock_lease` | [LockLease](#locklease) | How long the store's lock holds before it is taken as abandoned. |
| `store.lock_patience` | [LockPatience](#lockpatience) | How many times a lock with no time to judge its age by is found held, unchanged, before it is taken as abandoned. |
| `store.repository` | string | The repository, as `owner/name`; the run's own where absent. |
| `store.token` | table | The secret holding a token with the repository's Variables permission; the job's own token cannot write variables. |
| `store.token.secret` | string | The secret's name. |

## `templates`

The templates routes render with, by name.

| Setting | Type | What it sets |
|---------|------|--------------|
| `templates.<name>.body` | string | The body, in Markdown. |
| `templates.<name>.file` | string | A file beside the config holding the body. |
| `templates.<name>.title` | string | The title, on one line. |

## `version`

The version of the config: 1.

| Setting | Type | What it sets |
|---------|------|--------------|
| `version` | [Version](#version) | The version of the config: 1. |

## `views`

The views of open pull requests reports and `herald queue` show, by name; `queue`, over every source, where none replaces it.

| Setting | Type | What it sets |
|---------|------|--------------|
| `views.<name>.authors` | array of string | The authors it shows, by login, each a glob ignoring case; every one where empty. |
| `views.<name>.bots` | [Bots](#bots) | What it does with a bot's pull requests: "batch" where absent. |
| `views.<name>.bumps` | array of string | Its own bump patterns, read before Dependabot's and Renovate's, such as "chore: bump {package} to {to}". |

### `views.<name>.deciding`: herald's own rules, or the forge's own status

| Setting | Type | What it sets |
|---------|------|--------------|
| `views.<name>.deciding` | [DecidingName](#decidingname) | herald's own rules, or the forge's own status. |

### `views.<name>.deciding`: The labels maintainers set

| Setting | Type | What it sets |
|---------|------|--------------|
| `views.<name>.deciding` | table | The labels maintainers set. |
| `views.<name>.deciding.labels[].label` | string | The label, as the forge names it, ignoring case. |
| `views.<name>.deciding.labels[].state` | [Turn](#turn) | The state. |

| Setting | Type | What it sets |
|---------|------|--------------|
| `views.<name>.group_by` | [GroupBy](#groupby) | What it groups its rows by: "turn" where absent. |
| `views.<name>.labels` | array of string | The labels, any one of which a pull request it shows carries, each a glob ignoring case; every pull request where empty. |
| `views.<name>.most_open` | integer | The most open pull requests it lists of each repository: 500 where absent. |
| `views.<name>.most_pulls` | integer | The most it reads in full where each takes requests of its own, as on Forgejo and on GitHub without a token: 100 where absent; those past it read as unknown. |
| `views.<name>.repositories` | array of string | The repositories it shows, by full path, each a glob ignoring case; every one where empty. |
| `views.<name>.rules` | table | herald's own rules, each of which it may switch off. |
| `views.<name>.rules.approval` | boolean | One waits on review until it is approved. |
| `views.<name>.rules.changes` | boolean | One a reviewer asked to change waits on its author. |
| `views.<name>.rules.conflict` | boolean | One that conflicts waits on its author. |
| `views.<name>.rules.draft` | boolean | A draft waits on its author's readiness. |
| `views.<name>.rules.failed` | boolean | One whose check failed waits on its author. |
| `views.<name>.rules.pending` | boolean | One whose checks are running waits on them. |
| `views.<name>.rules.queued` | boolean | One in the merge queue waits on the queue. |
| `views.<name>.sources` | array of string | The sources it reads, each naming one repository on a forge; every such source where absent. |
| `views.<name>.stale` | [Duration](#duration) | How long one waits with nobody acting before it is stale: "14d" where absent. |
| `views.<name>.stale_only` | boolean | Whether it shows only the stale. |
| `views.<name>.states` | array of [Turn](#turn) | The states it shows, every one where empty. |

## Types

### AlertAction

What an alert did that is announced.

### Approve

Who decides whether a follow of herald's actors is taken.

### ArchiveAfter

How long a quiet thread stays open before Discord archives it: one of the four lengths Discord takes.

### Attempts

How many times the store tries a write that lost a race to an unrelated write, 3 by default.

### AttributeType

What type an attribute holds.

### Audience

Who sees an actor's Notes on the timelines. Followers see every Note.

### Bots

What a view does with a bot's pull requests.

### Colour

A colour, written "#rrggbb".

### Command

A command a member gives herald.

### Conclusion

How a finished run ended, in GitHub's words, which every forge's run is read as.

### Day

A day of the week, by its first three letters.

### DecidingName

What tells a view whose turn a pull request is, by name.

### Declared

Who may see a namespace's packages, where the source declares it.

### DiscordVia

How Discord carries commands to herald.

### Discovery

How an owner source keeps its membership.

### Domain

The Slack a workspace lives on: Slack's own, or Slack's government cloud.

### Duration

A length of time: a number and ms, s, m, h, d or w, as "14d".

### Edits

Which announcements a route edits the post of, when the forge changes what it announced.

### GroupBy

What a view groups its rows by.

### GroupLate

What a route does with a member that comes after its group was announced: once a held group closed to be announced, or once an edited group's window passed.

### GroupMode

How a route announces a group.

### GroupUntil

When a held group is announced.

### Guard

What doctor does with a config that takes something private to a public place.

### IssueAction

What an issue did that is announced.

### Keep

A length of time, or "forever".

### KeyCache

Where `herald server` keeps the keys inbound activities are checked with.

### Kind

What kind of thing an event announces: release, package-version, feed-entry, message, run, alert, or a kind the config declares.

### LateRelease

What a route does with a Release whose tag it already announced.

### LockLease

How long the store's lock holds before it is taken as abandoned, "10m" by default and at most "40m".

### LockPatience

How many times a lock with no time to judge its age by is found held, unchanged, before it is taken as abandoned, 10 by default.

### MatrixVia

How the homeserver carries commands to herald.

### MemberHooks

Who registers a project webhook on each member of a GitLab owner.

### Mode

Whom herald's actors federate with.

### OneCharacter

One character, as a config writes it.

### PackageVisibility

Who may see a package's version where its own visibility and its repository's differ.

### PinLookups

How many pins the destination looks up, one request each, as it finds the route's earlier pins, 25 by default.

### Pinning

How a route keeps its newest announcement pinned: each pin unpins the route's earlier one, found as this says (ADR-0011).

### Prefer

Which of a tag and its Release a source announces where it announces both.

### PrereleaseRule

How a source tells a pre-release.

### Prereleases

Which pre-releases a route takes.

### PullAction

What a pull request did that is announced.

### Rate

How many in a length of time: a number, a slash and s, m, h or d, as "120/m".

### ReadBack

How many pages of its own posts the destination reads back, 5 by default.

### RecordKind

A kind of record a store may keep beside its ledger, cursors, schedules and deliveries, which every store that keeps records keeps.

### Replies

Who sees herald's answers to `latest` and `changelog`; the answers to `subscribe` and `unsubscribe` are always the asker's alone.

### RepositoryVisibility

Who may see a repository.

### Rotation

Whether a peer's own signed statement may replace the keys pinned for it.

### RunAction

What a run did that is announced.

### RunFrom

What a run is read from.

### Severity

How severe an alert is, in lowercase on every forge.

### Signal

What a source announces of a repository.

### Size

A number of bytes: a number and KiB or MiB, as "1MiB".

### SlackVia

How Slack carries commands to herald.

### TagOrdering

How Docker Hub lists a repository's tags.

### TokenHash

### Tool

What raised an alert, in words every forge's alerts are read as.

### Transport

How an owner's members are polled.

### Turn

Whose turn an open pull request is.

### Values

A property's values: one, or any of several.

### VendorListing

How a Packagist vendor's packages are listed.

### Verify

Which envelopes a pull takes.

### Version

### Visibility

Who sees a status.

### Withdraw

What a route does with an announcement whose release the forge deleted (ADR-0048).
