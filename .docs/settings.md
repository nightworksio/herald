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
| `apps.<name>.projects` | array of string | The projects it may announce: at least one. |
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
| `destinations.<name>.allow` | array of string | The servers and actors an `allowlist` federates with, written as `block` writes them. |
| `destinations.<name>.approve` | [Approve](#approve) | Who takes a follow: `auto`, herald by the lists and caps, or `manual`, the operator through `herald followers`. |
| `destinations.<name>.block` | array of string | The servers and actors its actors never take a follow from or deliver to: each a domain, naming its subdomains too, or an actor's id. |
| `destinations.<name>.deliveries` | integer | How many of an announcement's followers' inboxes it delivers to at once, by every command that announces; 8 where unset. |
| `destinations.<name>.domain` | string | The domain herald serves its actors on, such as `herald.example`: a host alone, with no scheme, port or path. |
| `destinations.<name>.followers` | table | How many may follow each actor, in all and from any one server; a follow past either is answered with a `Reject`. |
| `destinations.<name>.followers.most` | integer | The most followers one actor keeps; no limit where unset. |
| `destinations.<name>.followers.per_domain` | integer | The most followers one actor keeps from any one server; no limit where unset. |
| `destinations.<name>.key` | table | The secret holding the actors' RSA private key, as PKCS#8 PEM, such as `openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048` writes. |
| `destinations.<name>.key.secret` | string | The secret's name. |
| `destinations.<name>.kind` | string |  |
| `destinations.<name>.lists[].file` | string | The file, as a path beside `herald.toml`. |
| `destinations.<name>.lists[].sha256` | string | The SHA-256 of the file's text, in hex: a list that changes is not read until its new digest is pinned. |
| `destinations.<name>.mode` | [Mode](#mode) | Whom its actors federate with: `open`, everyone `block` does not name, or `allowlist`, only those `allow` names. |
| `destinations.<name>.outbox_page` | integer | How many Notes one page of each actor's outbox holds, from 1 to 80; 20 where unset. |
| `destinations.<name>.symbols` | table | The characters its posts write where its markup has none: a task done and to do, a rule and a text list's bullet. |
| `destinations.<name>.symbols.bullet` | [OneCharacter](#onecharacter) | A list's bullet where a list is written as text: `•` in Slack and `-` in plain text where unset. |
| `destinations.<name>.symbols.done` | [OneCharacter](#onecharacter) | A task done; `☑` where unset. |
| `destinations.<name>.symbols.rule` | [OneCharacter](#onecharacter) | A rule's character, drawn three wide; `─` where unset. |
| `destinations.<name>.symbols.todo` | [OneCharacter](#onecharacter) | A task to do; `☐` where unset. |
| `destinations.<name>.waits` | table | How long it waits before sending again where an inbox answers that it is unwell or limited and names no wait. |
| `destinations.<name>.waits.limited` | [Duration](#duration) | After the platform answers that herald sends too often; the platform's own window where unset: "5m" on Mastodon, "5s" elsewhere. |
| `destinations.<name>.waits.outage` | [Duration](#duration) | After the platform answers that it is unwell; "5s" where unset. |

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

### `kinds.<name>.links[]`: A link whose button is labelled by its name.

| Setting | Type | What it sets |
|---------|------|--------------|
| `kinds.<name>.links[]` | string | A link whose button is labelled by its name. |

### `kinds.<name>.links[]`: A link and its button's label.

| Setting | Type | What it sets |
|---------|------|--------------|
| `kinds.<name>.links[]` | table | A link and its button's label. |
| `kinds.<name>.links[].label` | string | Its button's label. |
| `kinds.<name>.links[].name` | string | Its name, as `event.links.<name>` reads it. |

| Setting | Type | What it sets |
|---------|------|--------------|
| `kinds.<name>.versioned` | boolean | Whether its events carry a version, as `attributes.version`, and may be pre-releases, as `attributes.prerelease`. |

## `markers`

The installation's marker key.

| Setting | Type | What it sets |
|---------|------|--------------|
| `markers.key` | table | The secret holding the marker key, such as `HERALD_MARKER_SECRET`. |
| `markers.key.secret` | string | The secret's name. |

## `render`

How far a template may go as it renders.

| Setting | Type | What it sets |
|---------|------|--------------|
| `render.fuel` | integer | The fuel one render may burn; 100000 where absent, and at most 400000. |
| `render.output` | [Size](#size) | The most it may write; "256KiB" where absent, and at most "1MiB". |
| `render.recursion` | integer | How deep its loops and calls may nest; 16 where absent, and at most 64. |

## `routes`

Which events go where, rendered how.

### `routes[]`: A route to a Discord destination.

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].channel` | string | The channel's id. A bot needs one; a webhook posts where it was made. |
| `routes[].crosspost` | boolean | Whether to crosspost each announcement. |
| `routes[].destination` | string | The destination it announces to. |
| `routes[].forum` | boolean | Whether the channel is a forum, where each announcement is a post of its own. |
| `routes[].id` | string | The route's stable id, part of every announcement's key. |
| `routes[].kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pinning](#pinning) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].subscribe` | table | The opt-in role members subscribe to, where the route offers one. |
| `routes[].subscribe.button` | boolean | Whether each announcement carries a button that subscribes, or unsubscribes a member already subscribed. |
| `routes[].subscribe.role` | string | The opt-in role `subscribe` gives a member and `unsubscribe` takes away, by id. The route mentions it with its announcements. |
| `routes[].template` | string | The template it renders with. |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |

### `routes[]`: A route to a Mastodon destination.

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].content_warning` | string | The content warning each status is shown behind. |
| `routes[].crosspost` | boolean | Whether to crosspost each announcement. |
| `routes[].destination` | string | The destination it announces to. |
| `routes[].id` | string | The route's stable id, part of every announcement's key. |
| `routes[].kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `routes[].language` | string | The language each status is written in, as an ISO 639 code. |
| `routes[].limit` | integer | The most characters the server takes in a status, Mastodon's own 500 where it is not given. |
| `routes[].link_length` | integer | The characters the server counts a link as, however long it is: its `characters_reserved_per_url`, Mastodon's own 23 where unset. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pinning](#pinning) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with. |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].title` | integer | The longest title, a status's first line, in characters; 100 where unset. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].visibility` | [Visibility](#visibility) | Who sees each status. |

### `routes[]`: A route to a Matrix destination.

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].crosspost` | boolean | Whether to crosspost each announcement. |
| `routes[].destination` | string | The destination it announces to. |
| `routes[].id` | string | The route's stable id, part of every announcement's key. |
| `routes[].kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pinning](#pinning) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].room` | string | The room's id, such as `!abcdefg:matrix.org`, which the bot has joined. |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with. |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |

### `routes[]`: A route to a Slack destination.

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].channel` | string | The channel's id, such as `C0123456789`, which the bot has joined. A bot needs one; a webhook posts where it was made. |
| `routes[].crosspost` | boolean | Whether to crosspost each announcement. |
| `routes[].destination` | string | The destination it announces to. |
| `routes[].id` | string | The route's stable id, part of every announcement's key. |
| `routes[].kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pinning](#pinning) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].subscribe` | table | How the route offers subscribing, where it mentions its subscribers. |
| `routes[].subscribe.button` | boolean | Whether each announcement carries a button that subscribes, or unsubscribes a member already subscribed. |
| `routes[].template` | string | The template it renders with. |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |

### `routes[]`: A route announcing as one of herald's own ActivityPub actors.

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].actor` | string | The actor the route announces as, such as `php-mutation-gate`, which is followed as `@php-mutation-gate@<domain>`. |
| `routes[].content_warning` | string | The content warning each Note is shown behind. |
| `routes[].crosspost` | boolean | Whether to crosspost each announcement. |
| `routes[].destination` | string | The destination it announces to. |
| `routes[].id` | string | The route's stable id, part of every announcement's key. |
| `routes[].kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `routes[].language` | string | The language each Note is written in, as an ISO 639 code. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pinning](#pinning) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with. |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].visibility` | [Audience](#audience) | Who sees each Note on the timelines. |

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

## `sources`

What herald listens to, by name.

### `sources.<name>` of kind `github`

A GitHub repository's releases.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A GitHub repository's releases. |
| `sources.<name>.api` | string | The API's base URL, for a forge that is not GitHub, GitLab.com or Codeberg: a GitHub Enterprise Server's `https://ghe.example/api/v3`, a GHE.com tenant's `https://api.acme.ghe.com`, a self-managed or Dedicated GitLab's `https://gitlab.example/api/v4`, or a Forgejo's `https://forge.example/api/v1`. Where absent, it is the CI run's own when announcing the run's repository, and otherwise the forge's public instance. |
| `sources.<name>.backfill` | integer | How many of the newest releases the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.kind` | string |  |
| `sources.<name>.page` | integer | How many of the newest releases one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB" where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.repository` | string | The repository, as `owner/name`, or a GitLab project's full path, such as `group/subgroup/name`. |
| `sources.<name>.token` | table | A token that reaches a private repository or raises the API's rate limit, such as the run's `GITHUB_TOKEN`. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.webhook` | table | The secret the forge signs its webhook deliveries to `herald server` with: GitHub's and Forgejo's webhook secret, or GitLab's signing token (ADR-0007). |
| `sources.<name>.webhook.secret` | string | The secret's name. |

### `sources.<name>` of kind `gitlab`

A GitLab project's releases.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A GitLab project's releases. |
| `sources.<name>.api` | string | The API's base URL, for a forge that is not GitHub, GitLab.com or Codeberg: a GitHub Enterprise Server's `https://ghe.example/api/v3`, a GHE.com tenant's `https://api.acme.ghe.com`, a self-managed or Dedicated GitLab's `https://gitlab.example/api/v4`, or a Forgejo's `https://forge.example/api/v1`. Where absent, it is the CI run's own when announcing the run's repository, and otherwise the forge's public instance. |
| `sources.<name>.backfill` | integer | How many of the newest releases the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.kind` | string |  |
| `sources.<name>.page` | integer | How many of the newest releases one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB" where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.repository` | string | The repository, as `owner/name`, or a GitLab project's full path, such as `group/subgroup/name`. |
| `sources.<name>.token` | table | A token that reaches a private repository or raises the API's rate limit, such as the run's `GITHUB_TOKEN`. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.webhook` | table | The secret the forge signs its webhook deliveries to `herald server` with: GitHub's and Forgejo's webhook secret, or GitLab's signing token (ADR-0007). |
| `sources.<name>.webhook.secret` | string | The secret's name. |

### `sources.<name>` of kind `forgejo`

A Forgejo or Gitea repository's releases.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A Forgejo or Gitea repository's releases. |
| `sources.<name>.api` | string | The API's base URL, for a forge that is not GitHub, GitLab.com or Codeberg: a GitHub Enterprise Server's `https://ghe.example/api/v3`, a GHE.com tenant's `https://api.acme.ghe.com`, a self-managed or Dedicated GitLab's `https://gitlab.example/api/v4`, or a Forgejo's `https://forge.example/api/v1`. Where absent, it is the CI run's own when announcing the run's repository, and otherwise the forge's public instance. |
| `sources.<name>.backfill` | integer | How many of the newest releases the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.kind` | string |  |
| `sources.<name>.page` | integer | How many of the newest releases one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB" where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.repository` | string | The repository, as `owner/name`, or a GitLab project's full path, such as `group/subgroup/name`. |
| `sources.<name>.token` | table | A token that reaches a private repository or raises the API's rate limit, such as the run's `GITHUB_TOKEN`. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.webhook` | table | The secret the forge signs its webhook deliveries to `herald server` with: GitHub's and Forgejo's webhook secret, or GitLab's signing token (ADR-0007). |
| `sources.<name>.webhook.secret` | string | The secret's name. |

### `sources.<name>` of kind `packagist`

A package's versions on Packagist.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A package's versions on Packagist. |
| `sources.<name>.api` | string | The registry's base URL, for a mirror or a private registry that answers as the public one does; the public registry's where absent. |
| `sources.<name>.backfill` | integer | How many of the newest versions the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.kind` | string |  |
| `sources.<name>.ordering` | [TagOrdering](#tagordering) | How Docker Hub lists a repository's tags for a poll: `last_updated`, the most recently pushed first, where unset, or `name`. |
| `sources.<name>.package` | string | The package, as the registry names it: `vendor/name` on Packagist, a crate's name, an npm package's name such as `@scope/name`, or a Docker Hub repository such as `library/caddy`, where an official image may leave out `library/`. |
| `sources.<name>.page` | integer | How many of the newest versions one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB", "16MiB" on Packagist and "32MiB" on npm, where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.token` | table | A token for a private package: npm's, sent as a bearer token; Private Packagist's Composer token, sent as HTTP Basic with the user `token`; or a Docker Hub personal access token, exchanged with `username` for a bearer token. crates.io takes none. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.username` | string | The Docker Hub account `token` belongs to. |

### `sources.<name>` of kind `crates-io`

A crate's versions on crates.io.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A crate's versions on crates.io. |
| `sources.<name>.api` | string | The registry's base URL, for a mirror or a private registry that answers as the public one does; the public registry's where absent. |
| `sources.<name>.backfill` | integer | How many of the newest versions the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.kind` | string |  |
| `sources.<name>.ordering` | [TagOrdering](#tagordering) | How Docker Hub lists a repository's tags for a poll: `last_updated`, the most recently pushed first, where unset, or `name`. |
| `sources.<name>.package` | string | The package, as the registry names it: `vendor/name` on Packagist, a crate's name, an npm package's name such as `@scope/name`, or a Docker Hub repository such as `library/caddy`, where an official image may leave out `library/`. |
| `sources.<name>.page` | integer | How many of the newest versions one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB", "16MiB" on Packagist and "32MiB" on npm, where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.token` | table | A token for a private package: npm's, sent as a bearer token; Private Packagist's Composer token, sent as HTTP Basic with the user `token`; or a Docker Hub personal access token, exchanged with `username` for a bearer token. crates.io takes none. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.username` | string | The Docker Hub account `token` belongs to. |

### `sources.<name>` of kind `npm`

A package's versions on the npm registry.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A package's versions on the npm registry. |
| `sources.<name>.api` | string | The registry's base URL, for a mirror or a private registry that answers as the public one does; the public registry's where absent. |
| `sources.<name>.backfill` | integer | How many of the newest versions the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.kind` | string |  |
| `sources.<name>.ordering` | [TagOrdering](#tagordering) | How Docker Hub lists a repository's tags for a poll: `last_updated`, the most recently pushed first, where unset, or `name`. |
| `sources.<name>.package` | string | The package, as the registry names it: `vendor/name` on Packagist, a crate's name, an npm package's name such as `@scope/name`, or a Docker Hub repository such as `library/caddy`, where an official image may leave out `library/`. |
| `sources.<name>.page` | integer | How many of the newest versions one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB", "16MiB" on Packagist and "32MiB" on npm, where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.token` | table | A token for a private package: npm's, sent as a bearer token; Private Packagist's Composer token, sent as HTTP Basic with the user `token`; or a Docker Hub personal access token, exchanged with `username` for a bearer token. crates.io takes none. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.username` | string | The Docker Hub account `token` belongs to. |

### `sources.<name>` of kind `docker-hub`

A repository's tags on Docker Hub.

| Setting | Type | What it sets |
|---------|------|--------------|
| `sources.<name>` | table | A repository's tags on Docker Hub. |
| `sources.<name>.api` | string | The registry's base URL, for a mirror or a private registry that answers as the public one does; the public registry's where absent. |
| `sources.<name>.backfill` | integer | How many of the newest versions the first poll announces; none where absent (ADR-0003). |
| `sources.<name>.kind` | string |  |
| `sources.<name>.ordering` | [TagOrdering](#tagordering) | How Docker Hub lists a repository's tags for a poll: `last_updated`, the most recently pushed first, where unset, or `name`. |
| `sources.<name>.package` | string | The package, as the registry names it: `vendor/name` on Packagist, a crate's name, an npm package's name such as `@scope/name`, or a Docker Hub repository such as `library/caddy`, where an official image may leave out `library/`. |
| `sources.<name>.page` | integer | How many of the newest versions one poll reads; 30 where absent. |
| `sources.<name>.parse` | [Size](#size) | The most of an answer parsed; "4MiB", "16MiB" on Packagist and "32MiB" on npm, where absent, and at most four times that. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.token` | table | A token for a private package: npm's, sent as a bearer token; Private Packagist's Composer token, sent as HTTP Basic with the user `token`; or a Docker Hub personal access token, exchanged with `username` for a bearer token. crates.io takes none. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.username` | string | The Docker Hub account `token` belongs to. |

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

## Types

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

### Colour

A colour, written "#rrggbb".

### Command

A command a member gives herald.

### DiscordVia

How Discord carries commands to herald.

### Domain

The Slack a workspace lives on: Slack's own, or Slack's government cloud.

### Duration

A length of time: a number and ms, s, m, h, d or w, as "14d".

### Keep

A length of time, or "forever".

### KeyCache

Where `herald server` keeps the keys inbound activities are checked with.

### Kind

What kind of thing an event announces: release, package-version, feed-entry, message, or a kind the config declares.

### LockLease

How long the store's lock holds before it is taken as abandoned, "10m" by default and at most "40m".

### LockPatience

How many times a lock with no time to judge its age by is found held, unchanged, before it is taken as abandoned, 10 by default.

### MatrixVia

How the homeserver carries commands to herald.

### Mode

Whom herald's actors federate with.

### OneCharacter

One character, as a config writes it.

### PinLookups

How many pins the destination looks up, one request each, as it finds the route's earlier pins, 25 by default.

### Pinning

How a route keeps its newest announcement pinned: each pin unpins the route's earlier one, found as this says (ADR-0011).

### Prereleases

Which pre-releases a route takes.

### ReadBack

How many pages of its own posts the destination reads back, 5 by default.

### RecordKind

A kind of record a store may keep beside its ledger, cursors, schedules and deliveries, which every store that keeps records keeps.

### Replies

Who sees herald's answers to `latest` and `changelog`; the answers to `subscribe` and `unsubscribe` are always the asker's alone.

### Size

A number of bytes: a number and KiB or MiB, as "1MiB".

### SlackVia

How Slack carries commands to herald.

### TagOrdering

How Docker Hub lists a repository's tags.

### TokenHash



### Version



### Visibility

Who sees a status.
