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
| `apps.<name>.oidc` | array of tables | The ID tokens it takes as credentials: each the issuer of a CI run's ID token and what such a token must say, so a run of GitHub Actions or GitLab CI proves who it is and keeps no stored secret. |
| `apps.<name>.oidc[].algorithms` | array of [TokenAlgorithm](#tokenalgorithm) | The algorithms a token may be signed with; `["RS256"]` where absent. What its header names is never taken on its own. |
| `apps.<name>.oidc[].audience` | string | What a token's `aud` claim must name: this herald's `[server] url` where absent, else `https://` and its `[federation] domain`. Never an issuer, a peer's URL, nor a GitHub's default `<GitHub>/<owner>`, which every herald of the owner would take alike: github.com's, or the GHE.com enterprise's or GitHub Enterprise Server's the issuer is. |
| `apps.<name>.oidc[].claims` | table | Each claim a token must carry, by name, and the pattern its value must match, case and all: `*` stands for any run of characters and `?` for any one, neither reaching past a `/`. At least one binds who the run is: `repository_id`, `repository_owner_id`, `project_id` or `namespace_id` with no wildcard, `repository`, `repository_owner`, `project_path` or `namespace_path` with none before its first `/`, or `sub` as `repo:<owner>/...` or `project_path:<group>/...`, the owner whole; else a token of any repository at the issuer would be taken, as a GitHub repository shapes its own `sub`. Prefer an id a renamed or re-created repository never inherits; `herald doctor` names an entry that binds names alone, `sub` among them. |
| `apps.<name>.oidc[].claims.<name>` | string |  |
| `apps.<name>.oidc[].issuer` | string | The issuer, `https://`, exactly as its tokens' `iss` claim names it: `https://token.actions.githubusercontent.com` for GitHub Actions, with `/<enterprise>` after it where an enterprise of GitHub Enterprise Cloud issues its own; `https://token.actions.<enterprise>.ghe.com` on GHE.com; `https://<host>/_services/token` on GitHub Enterprise Server; `https://gitlab.com`, or the instance's URL on GitLab self-managed. |
| `apps.<name>.oidc[].jwks` | string | Where the issuer's keys are read, `https://`, in place of the `jwks_uri` its discovery document names on its own origin. |
| `apps.<name>.oidc[].keys` | array of object | The issuer's keys, as a JSON Web Key Set's `keys` lists them, pinned so they are never read from the network; each rotation is then the operator's to write. |
| `apps.<name>.oidc[].keys_for` | [Duration](#duration) | How long the issuer's keys, once read, are used before they are read again: "1h" where absent, and at most "1d". Keys that cannot be read again are not used past it. |
| `apps.<name>.oidc[].keys_retry` | [Duration](#duration) | How long after the issuer's keys were last fetched, or tried, a token naming a key they hold none of has them fetched again: "1m" where absent, from "10s" to `keys_for`. A rotated key is taken within it, and however many tokens name unknown keys, the issuer is asked at most once in it. |
| `apps.<name>.oidc[].lifetime` | [Duration](#duration) | The longest a token may live, from its `iat` to its `exp`: "1h" where absent, at most "1d". A longer-lived token is refused, so a captured one is worth nothing past it; GitLab CI's tokens live as long as their job's timeout. |
| `apps.<name>.oidc[].skew` | [Duration](#duration) | How far a token's `exp`, `nbf` and `iat` may stand from this herald's clock: "1m" where absent, at most "5m". |
| `apps.<name>.oidc[].takes` | array of [Taking](#taking) | What its tokens may do: `"announce"` through the HTTP API, `"push"` into a herald source as a name the app stands for, or both. Where absent, what the app does; an app naming both projects and peers says it on each entry, so no token is taken by both unless its entry grants both. |
| `apps.<name>.peers` | array of string | The names it stands for when a run holding one of its ID tokens pushes into a herald source, such as a repository's herald running in CI, which has no URL or keys of its own. A herald source grants each name in its `peers`, as it grants a peer; no name is a table of `[peers]` or another app's. |
| `apps.<name>.projects` | array of string | The projects it may announce, each a glob ignoring case, such as `"plugin-*"`; at least one, unless it stands for `peers` alone. A project named with no wildcard is one the app announces as; a glob only scopes what it may announce, and takes no project from a source or member. |
| `apps.<name>.replay` | [Replay](#replay) | How often one of its ID tokens may be presented: `"once"`, where the store keeps deliveries, remembers each token taken until it expires and refuses it a second time, so a captured token is worth nothing once used; `"until-expiry"` takes it as often as it comes until it expires. `"once"` where absent and the store keeps deliveries, else `"until-expiry"`, which `herald doctor` names. |
| `apps.<name>.routes` | array of string | The routes it announces to, each naming the app among its sources; every such route where it names none. |
| `apps.<name>.tokens` | array of [TokenHash](#tokenhash) | The SHA-256 of each of the app's tokens, written `sha256:` and 64 lowercase hex digits, as `herald token new` prints it. A token is rotated by adding the new one before the old one is removed. |

## `boards`

The living boards, by name: each a report and a route that keeps one post of it, edited each window and pinned.

### `boards.<name>`: A route to a Discord destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `boards.<name>` | table | A route to a Discord destination. |
| `boards.<name>.area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
| `boards.<name>.at` | string | The wall-clock time a day's first window begins at, as "HH:MM": "09:00" where absent. |
| `boards.<name>.channel` | string | The channel's id. A bot needs one; a webhook posts where it was made. |
| `boards.<name>.crosspost` | boolean | Whether to crosspost each announcement. |
| `boards.<name>.days` | array of [Day](#day) | The days windows begin on, every day where empty. |
| `boards.<name>.destination` | string | The destination it announces to. |
| `boards.<name>.edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `boards.<name>.every` | [Duration](#duration) | How long each window lasts within a day, from "1m" to "1d": "1d" where absent. |
| `boards.<name>.forum` | boolean | Whether the channel is a forum, where each announcement is a post of its own. |
| `boards.<name>.group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `boards.<name>.group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `boards.<name>.group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `boards.<name>.group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `boards.<name>.group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `boards.<name>.group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `boards.<name>.group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `boards.<name>.group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `boards.<name>.kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `boards.<name>.late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `boards.<name>.match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `boards.<name>.match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `boards.<name>.match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `boards.<name>.match.attributes.<name>` | array of string |  |
| `boards.<name>.match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `boards.<name>.match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `boards.<name>.match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `boards.<name>.match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `boards.<name>.match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `boards.<name>.match.properties.<name>` | array of string |  |
| `boards.<name>.match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `boards.<name>.match.topics` | array of string | The topics, any one of which a repository holds. |
| `boards.<name>.mentions` | table | Who it mentions. |
| `boards.<name>.mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `boards.<name>.mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `boards.<name>.pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `boards.<name>.prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `boards.<name>.private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `boards.<name>.subscribe` | table | The opt-in role members subscribe to, where the route offers one. |
| `boards.<name>.subscribe.button` | boolean | Whether each announcement carries a button that subscribes, or unsubscribes a member already subscribed. |
| `boards.<name>.subscribe.role` | string | The opt-in role `subscribe` gives a member and `unsubscribe` takes away, by id. The route mentions it with its announcements. |
| `boards.<name>.template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `boards.<name>.templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `boards.<name>.templates.<name>` | string |  |
| `boards.<name>.thread` | table | The thread to open for each announcement. |
| `boards.<name>.thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `boards.<name>.thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `boards.<name>.thread.name` | string | The template of the thread's name. |
| `boards.<name>.versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `boards.<name>.view` | string | The view it shows: "queue", every source's open pull requests, where absent. |
| `boards.<name>.viewers` | array of string | The ids of the roles, and of any member a permission overwrite of their own lets in, expected to view the channel: an advisories section is sent there only where every role but `@everyone` that may view it is listed here, an Administrator role and the bot's own role included, and no member it does not list is let in by an overwrite; the server's owner sees every channel and is not checked (ADR-0067). |
| `boards.<name>.vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `boards.<name>.withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |
| `boards.<name>.zone` | string | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam": "UTC" where absent. |

### `boards.<name>`: A route to a Mastodon destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `boards.<name>` | table | A route to a Mastodon destination. |
| `boards.<name>.area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
| `boards.<name>.at` | string | The wall-clock time a day's first window begins at, as "HH:MM": "09:00" where absent. |
| `boards.<name>.content_warning` | string | The content warning each status is shown behind. |
| `boards.<name>.crosspost` | boolean | Whether to crosspost each announcement. |
| `boards.<name>.days` | array of [Day](#day) | The days windows begin on, every day where empty. |
| `boards.<name>.destination` | string | The destination it announces to. |
| `boards.<name>.edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `boards.<name>.every` | [Duration](#duration) | How long each window lasts within a day, from "1m" to "1d": "1d" where absent. |
| `boards.<name>.group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `boards.<name>.group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `boards.<name>.group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `boards.<name>.group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `boards.<name>.group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `boards.<name>.group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `boards.<name>.group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `boards.<name>.group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `boards.<name>.kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `boards.<name>.language` | string | The language each status is written in, as an ISO 639 code. |
| `boards.<name>.late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `boards.<name>.limit` | integer | The most characters the server takes in a status, Mastodon's own 500 where it is not given. |
| `boards.<name>.link_length` | integer | The characters the server counts a link as, however long it is: its `characters_reserved_per_url`, Mastodon's own 23 where unset. |
| `boards.<name>.match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `boards.<name>.match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `boards.<name>.match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `boards.<name>.match.attributes.<name>` | array of string |  |
| `boards.<name>.match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `boards.<name>.match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `boards.<name>.match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `boards.<name>.match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `boards.<name>.match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `boards.<name>.match.properties.<name>` | array of string |  |
| `boards.<name>.match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `boards.<name>.match.topics` | array of string | The topics, any one of which a repository holds. |
| `boards.<name>.mentions` | table | Who it mentions. |
| `boards.<name>.mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `boards.<name>.mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `boards.<name>.pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `boards.<name>.prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `boards.<name>.private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `boards.<name>.template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `boards.<name>.templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `boards.<name>.templates.<name>` | string |  |
| `boards.<name>.thread` | table | The thread to open for each announcement. |
| `boards.<name>.thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `boards.<name>.thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `boards.<name>.thread.name` | string | The template of the thread's name. |
| `boards.<name>.title` | integer | The longest title, a status's first line, in characters; 100 where unset. |
| `boards.<name>.versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `boards.<name>.view` | string | The view it shows: "queue", every source's open pull requests, where absent. |
| `boards.<name>.visibility` | [Visibility](#visibility) | Who sees each status. |
| `boards.<name>.vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `boards.<name>.withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |
| `boards.<name>.zone` | string | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam": "UTC" where absent. |

### `boards.<name>`: A route to a Matrix destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `boards.<name>` | table | A route to a Matrix destination. |
| `boards.<name>.area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
| `boards.<name>.at` | string | The wall-clock time a day's first window begins at, as "HH:MM": "09:00" where absent. |
| `boards.<name>.crosspost` | boolean | Whether to crosspost each announcement. |
| `boards.<name>.days` | array of [Day](#day) | The days windows begin on, every day where empty. |
| `boards.<name>.destination` | string | The destination it announces to. |
| `boards.<name>.edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `boards.<name>.every` | [Duration](#duration) | How long each window lasts within a day, from "1m" to "1d": "1d" where absent. |
| `boards.<name>.group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `boards.<name>.group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `boards.<name>.group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `boards.<name>.group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `boards.<name>.group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `boards.<name>.group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `boards.<name>.group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `boards.<name>.group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `boards.<name>.kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `boards.<name>.late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `boards.<name>.match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `boards.<name>.match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `boards.<name>.match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `boards.<name>.match.attributes.<name>` | array of string |  |
| `boards.<name>.match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `boards.<name>.match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `boards.<name>.match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `boards.<name>.match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `boards.<name>.match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `boards.<name>.match.properties.<name>` | array of string |  |
| `boards.<name>.match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `boards.<name>.match.topics` | array of string | The topics, any one of which a repository holds. |
| `boards.<name>.mentions` | table | Who it mentions. |
| `boards.<name>.mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `boards.<name>.mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `boards.<name>.pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `boards.<name>.prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `boards.<name>.private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `boards.<name>.room` | string | The room's id, such as `!abcdefg:matrix.org`, which the bot has joined. |
| `boards.<name>.template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `boards.<name>.templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `boards.<name>.templates.<name>` | string |  |
| `boards.<name>.thread` | table | The thread to open for each announcement. |
| `boards.<name>.thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `boards.<name>.thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `boards.<name>.thread.name` | string | The template of the thread's name. |
| `boards.<name>.versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `boards.<name>.view` | string | The view it shows: "queue", every source's open pull requests, where absent. |
| `boards.<name>.vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `boards.<name>.withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |
| `boards.<name>.zone` | string | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam": "UTC" where absent. |

### `boards.<name>`: A route to a Slack destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `boards.<name>` | table | A route to a Slack destination. |
| `boards.<name>.area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
| `boards.<name>.at` | string | The wall-clock time a day's first window begins at, as "HH:MM": "09:00" where absent. |
| `boards.<name>.channel` | string | The channel's id, such as `C0123456789`, which the bot has joined. A bot needs one; a webhook posts where it was made. |
| `boards.<name>.crosspost` | boolean | Whether to crosspost each announcement. |
| `boards.<name>.days` | array of [Day](#day) | The days windows begin on, every day where empty. |
| `boards.<name>.destination` | string | The destination it announces to. |
| `boards.<name>.edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `boards.<name>.every` | [Duration](#duration) | How long each window lasts within a day, from "1m" to "1d": "1d" where absent. |
| `boards.<name>.group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `boards.<name>.group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `boards.<name>.group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `boards.<name>.group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `boards.<name>.group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `boards.<name>.group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `boards.<name>.group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `boards.<name>.group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `boards.<name>.kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `boards.<name>.late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `boards.<name>.match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `boards.<name>.match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `boards.<name>.match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `boards.<name>.match.attributes.<name>` | array of string |  |
| `boards.<name>.match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `boards.<name>.match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `boards.<name>.match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `boards.<name>.match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `boards.<name>.match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `boards.<name>.match.properties.<name>` | array of string |  |
| `boards.<name>.match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `boards.<name>.match.topics` | array of string | The topics, any one of which a repository holds. |
| `boards.<name>.mentions` | table | Who it mentions. |
| `boards.<name>.mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `boards.<name>.mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `boards.<name>.pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `boards.<name>.prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `boards.<name>.private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `boards.<name>.subscribe` | table | How the route offers subscribing, where it mentions its subscribers. |
| `boards.<name>.subscribe.button` | boolean | Whether each announcement carries a button that subscribes, or unsubscribes a member already subscribed. |
| `boards.<name>.template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `boards.<name>.templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `boards.<name>.templates.<name>` | string |  |
| `boards.<name>.thread` | table | The thread to open for each announcement. |
| `boards.<name>.thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `boards.<name>.thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `boards.<name>.thread.name` | string | The template of the thread's name. |
| `boards.<name>.versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `boards.<name>.view` | string | The view it shows: "queue", every source's open pull requests, where absent. |
| `boards.<name>.vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `boards.<name>.withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |
| `boards.<name>.zone` | string | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam": "UTC" where absent. |

### `boards.<name>`: A route to a Microsoft Teams destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `boards.<name>` | table | A route to a Microsoft Teams destination. |
| `boards.<name>.area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
| `boards.<name>.at` | string | The wall-clock time a day's first window begins at, as "HH:MM": "09:00" where absent. |
| `boards.<name>.conversation` | string | The id of the channel or chat the bot posts to, such as `19:…@thread.tacv2`, where its app is installed. A bot needs one; a webhook posts where its workflow names. |
| `boards.<name>.crosspost` | boolean | Whether to crosspost each announcement. |
| `boards.<name>.days` | array of [Day](#day) | The days windows begin on, every day where empty. |
| `boards.<name>.destination` | string | The destination it announces to. |
| `boards.<name>.edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `boards.<name>.every` | [Duration](#duration) | How long each window lasts within a day, from "1m" to "1d": "1d" where absent. |
| `boards.<name>.group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `boards.<name>.group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `boards.<name>.group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `boards.<name>.group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `boards.<name>.group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `boards.<name>.group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `boards.<name>.group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `boards.<name>.group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `boards.<name>.kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `boards.<name>.late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `boards.<name>.match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `boards.<name>.match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `boards.<name>.match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `boards.<name>.match.attributes.<name>` | array of string |  |
| `boards.<name>.match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `boards.<name>.match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `boards.<name>.match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `boards.<name>.match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `boards.<name>.match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `boards.<name>.match.properties.<name>` | array of string |  |
| `boards.<name>.match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `boards.<name>.match.topics` | array of string | The topics, any one of which a repository holds. |
| `boards.<name>.mentions` | table | Who it mentions. |
| `boards.<name>.mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `boards.<name>.mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `boards.<name>.pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `boards.<name>.prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `boards.<name>.private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `boards.<name>.template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `boards.<name>.templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `boards.<name>.templates.<name>` | string |  |
| `boards.<name>.thread` | table | The thread to open for each announcement. |
| `boards.<name>.thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `boards.<name>.thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `boards.<name>.thread.name` | string | The template of the thread's name. |
| `boards.<name>.threads` | boolean | Whether the bot posts each overflow part as a reply in its post's thread, through Microsoft's `replyChains`, which Microsoft marks as a preview; each part is posted on its own where unset. |
| `boards.<name>.versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `boards.<name>.view` | string | The view it shows: "queue", every source's open pull requests, where absent. |
| `boards.<name>.vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `boards.<name>.withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |
| `boards.<name>.zone` | string | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam": "UTC" where absent. |

### `boards.<name>`: A route announcing as one of herald's own ActivityPub actors

| Setting | Type | What it sets |
|---------|------|--------------|
| `boards.<name>` | table | A route announcing as one of herald's own ActivityPub actors. |
| `boards.<name>.actor` | string | The actor the route announces as, such as `php-mutation-gate`, which is followed as `@php-mutation-gate@<domain>`; or a template naming each project's own actor from `event.project`, such as `{{ event.project }}`. A project whose name is no actor's name is not announced on the route. |
| `boards.<name>.area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
| `boards.<name>.at` | string | The wall-clock time a day's first window begins at, as "HH:MM": "09:00" where absent. |
| `boards.<name>.content_warning` | string | The content warning each Note is shown behind. |
| `boards.<name>.crosspost` | boolean | Whether to crosspost each announcement. |
| `boards.<name>.days` | array of [Day](#day) | The days windows begin on, every day where empty. |
| `boards.<name>.destination` | string | The destination it announces to. |
| `boards.<name>.edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `boards.<name>.every` | [Duration](#duration) | How long each window lasts within a day, from "1m" to "1d": "1d" where absent. |
| `boards.<name>.group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `boards.<name>.group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `boards.<name>.group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `boards.<name>.group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `boards.<name>.group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `boards.<name>.group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `boards.<name>.group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `boards.<name>.group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `boards.<name>.kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `boards.<name>.language` | string | The language each Note is written in, as an ISO 639 code. |
| `boards.<name>.late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `boards.<name>.match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `boards.<name>.match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `boards.<name>.match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `boards.<name>.match.attributes.<name>` | array of string |  |
| `boards.<name>.match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `boards.<name>.match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `boards.<name>.match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `boards.<name>.match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `boards.<name>.match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `boards.<name>.match.properties.<name>` | array of string |  |
| `boards.<name>.match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `boards.<name>.match.topics` | array of string | The topics, any one of which a repository holds. |
| `boards.<name>.mentions` | table | Who it mentions. |
| `boards.<name>.mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `boards.<name>.mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `boards.<name>.pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `boards.<name>.prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `boards.<name>.private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `boards.<name>.template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `boards.<name>.templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `boards.<name>.templates.<name>` | string |  |
| `boards.<name>.thread` | table | The thread to open for each announcement. |
| `boards.<name>.thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `boards.<name>.thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `boards.<name>.thread.name` | string | The template of the thread's name. |
| `boards.<name>.versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `boards.<name>.view` | string | The view it shows: "queue", every source's open pull requests, where absent. |
| `boards.<name>.visibility` | [Audience](#audience) | Who sees each Note on the timelines. |
| `boards.<name>.vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `boards.<name>.withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |
| `boards.<name>.zone` | string | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam": "UTC" where absent. |

### `boards.<name>`: A route publishing its events to a feed of herald's own

| Setting | Type | What it sets |
|---------|------|--------------|
| `boards.<name>` | table | A route publishing its events to a feed of herald's own. |
| `boards.<name>.area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
| `boards.<name>.at` | string | The wall-clock time a day's first window begins at, as "HH:MM": "09:00" where absent. |
| `boards.<name>.crosspost` | boolean | Whether to crosspost each announcement. |
| `boards.<name>.days` | array of [Day](#day) | The days windows begin on, every day where empty. |
| `boards.<name>.destination` | string | The destination it announces to. |
| `boards.<name>.edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `boards.<name>.every` | [Duration](#duration) | How long each window lasts within a day, from "1m" to "1d": "1d" where absent. |
| `boards.<name>.group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `boards.<name>.group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `boards.<name>.group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `boards.<name>.group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `boards.<name>.group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `boards.<name>.group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `boards.<name>.group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `boards.<name>.group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `boards.<name>.kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `boards.<name>.late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `boards.<name>.match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `boards.<name>.match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `boards.<name>.match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `boards.<name>.match.attributes.<name>` | array of string |  |
| `boards.<name>.match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `boards.<name>.match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `boards.<name>.match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `boards.<name>.match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `boards.<name>.match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `boards.<name>.match.properties.<name>` | array of string |  |
| `boards.<name>.match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `boards.<name>.match.topics` | array of string | The topics, any one of which a repository holds. |
| `boards.<name>.mentions` | table | Who it mentions. |
| `boards.<name>.mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `boards.<name>.mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `boards.<name>.pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `boards.<name>.prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `boards.<name>.private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `boards.<name>.template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `boards.<name>.templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `boards.<name>.templates.<name>` | string |  |
| `boards.<name>.thread` | table | The thread to open for each announcement. |
| `boards.<name>.thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `boards.<name>.thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `boards.<name>.thread.name` | string | The template of the thread's name. |
| `boards.<name>.versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `boards.<name>.view` | string | The view it shows: "queue", every source's open pull requests, where absent. |
| `boards.<name>.vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `boards.<name>.withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |
| `boards.<name>.zone` | string | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam": "UTC" where absent. |

### `boards.<name>`: A route pushing its events to a peer's herald source

| Setting | Type | What it sets |
|---------|------|--------------|
| `boards.<name>` | table | A route pushing its events to a peer's herald source. |
| `boards.<name>.area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
| `boards.<name>.at` | string | The wall-clock time a day's first window begins at, as "HH:MM": "09:00" where absent. |
| `boards.<name>.crosspost` | boolean | Whether to crosspost each announcement. |
| `boards.<name>.days` | array of [Day](#day) | The days windows begin on, every day where empty. |
| `boards.<name>.destination` | string | The destination it announces to. |
| `boards.<name>.edits` | [Edits](#edits) | Which changed announcements it edits the post of: none, or a Release whose name or notes the forge says were edited (`release`). |
| `boards.<name>.every` | [Duration](#duration) | How long each window lasts within a day, from "1m" to "1d": "1d" where absent. |
| `boards.<name>.group` | table | How it gathers the events that share a key into one announcement; each event is announced alone where absent. |
| `boards.<name>.group.key` | string | The template of the key events are grouped by, on one line; an event whose key renders empty, or longer than 256 bytes, is announced alone. |
| `boards.<name>.group.late` | [GroupLate](#grouplate) | What becomes of a member that comes after its group was announced: announced alone (`post`), edited into the group's post where the destination edits one and announced alone otherwise (`edit`), or left unannounced (`skip`). |
| `boards.<name>.group.lead` | array of string | The projects whose event leads a group, as globs ignoring case, the first that matches a member first; the first member leads where none does. |
| `boards.<name>.group.mode` | [GroupMode](#groupmode) | How the group is announced: held until it is due, then announced as one (`hold`), or the first member announced at once and its post edited as each other comes within the window (`edit`), where the destination edits a post. |
| `boards.<name>.group.most` | integer | How many members a group takes, each member more announced alone; 25 where absent, and at most 100. |
| `boards.<name>.group.until` | [GroupUntil](#groupuntil) | When a held group is due: when its window passes (`window`), or as soon as the member that leads it comes (`lead`). |
| `boards.<name>.group.window` | [Duration](#duration) | How long a group gathers its members, from when herald sees the first; "30m" where absent, and at most "7d". |
| `boards.<name>.kinds` | array of [Kind](#kind) | The kinds of event it takes; every kind where absent. |
| `boards.<name>.late_release` | [LateRelease](#laterelease) | What it does with a Release whose tag it already announced: edits the tag's post to it, or posts it anew where the destination cannot edit (`edit`); posts it anew (`post`); or leaves the tag's post (`skip`). |
| `boards.<name>.match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `boards.<name>.match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `boards.<name>.match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `boards.<name>.match.attributes.<name>` | array of string |  |
| `boards.<name>.match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `boards.<name>.match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `boards.<name>.match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `boards.<name>.match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `boards.<name>.match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `boards.<name>.match.properties.<name>` | array of string |  |
| `boards.<name>.match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `boards.<name>.match.topics` | array of string | The topics, any one of which a repository holds. |
| `boards.<name>.mentions` | table | Who it mentions. |
| `boards.<name>.mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `boards.<name>.mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `boards.<name>.pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `boards.<name>.prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `boards.<name>.private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `boards.<name>.template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `boards.<name>.templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `boards.<name>.templates.<name>` | string |  |
| `boards.<name>.thread` | table | The thread to open for each announcement. |
| `boards.<name>.thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `boards.<name>.thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `boards.<name>.thread.name` | string | The template of the thread's name. |
| `boards.<name>.versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `boards.<name>.view` | string | The view it shows: "queue", every source's open pull requests, where absent. |
| `boards.<name>.vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `boards.<name>.withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |
| `boards.<name>.zone` | string | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam": "UTC" where absent. |

## `cli`

What herald's commands print.

| Setting | Type | What it sets |
|---------|------|--------------|
| `cli.private` | boolean | Whether `herald queue` and `herald rerun` print the rows and plans of private and internal repositories, and `herald poll` and `herald announce` name their events by version or title, without `--private`: false where absent, so rows are withheld and counted and events named "(private)". Set it only where their output stays private: a CI job's log on a public repository is public. Advisories are printed with `--private` alone, whatever it says. A server's log names such events "(private)" always. |

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
| `destinations.<name>.interactions.wording` | table | What it says back and how it describes its commands, phrase by phrase, each a template over `project`, `projects`, `named` and `command`; herald's own words for every phrase left out. |
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
| `destinations.<name>.interactions.wording` | table | What it says back and how it describes its commands, phrase by phrase, each a template over `project`, `projects`, `named` and `command`; herald's own words for every phrase left out. |
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
| `destinations.<name>.interactions.wording` | table | What it says back and how it describes its commands, phrase by phrase, each a template over `project`, `projects`, `named` and `command`; herald's own words for every phrase left out. |
| `destinations.<name>.interactions.wording.<name>` | string |  |
| `destinations.<name>.kind` | string |  |
| `destinations.<name>.org_shared` | [OrgShared](#orgshared) | How a private channel an Enterprise Grid organization shares with its other workspaces counts before an advisories section is sent there: as public, since members of every workspace of the organization may be in it (`public`, where unset), or as private (`private`), where the operator accepts that whole organization as its audience. |
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

### `destinations.<name>` of kind `teams`

Microsoft Teams, by Workflows webhook or as a bot.

| Setting | Type | What it sets |
|---------|------|--------------|
| `destinations.<name>` | table | Microsoft Teams, by Workflows webhook or as a bot. |
| `destinations.<name>.app_id` | string | A bot's Microsoft App ID: the application (client) ID of its Microsoft Entra app registration. A bot edits its posts, and replies in their threads where a route says so. |
| `destinations.<name>.app_password` | table | The secret holding the bot's client secret, which its app registration made. |
| `destinations.<name>.app_password.secret` | string | The secret's name. |
| `destinations.<name>.cloud` | [Cloud](#cloud) | The Microsoft cloud the bot is registered in: `public` where unset, `gcc-high` or `dod`. It names the hosts below where they are unset. |
| `destinations.<name>.interactions` | table | How the bot takes what Teams posts to it at herald's endpoint. |
| `destinations.<name>.interactions.via` | [TeamsVia](#teamsvia) | How Teams carries it. |
| `destinations.<name>.issuer` | string | The issuer each token vouching for an activity names, in place of its cloud's, such as `https://api.botframework.com`. |
| `destinations.<name>.kind` | string |  |
| `destinations.<name>.login` | string | The base URL of the sign-in service the bot asks for its token, in place of its cloud's, such as `https://login.microsoftonline.com`. |
| `destinations.<name>.openid` | string | The address of the Bot Framework's metadata document, in place of its cloud's, such as `https://login.botframework.com/v1/.well-known/openidconfiguration`: it names the keys that sign each token vouching for an activity Teams posts to the bot. |
| `destinations.<name>.scope` | string | The resource the bot's token is for, in place of its cloud's, such as `https://api.botframework.com`. |
| `destinations.<name>.service` | string | The service URL the bot posts through, in place of its cloud's, such as `https://smba.trafficmanager.net/teams/`. An activity Teams posts to the bot is taken only where the service URL it names is on this URL's origin. |
| `destinations.<name>.symbols` | table | The characters its posts write where its markup has none: a task done and to do, a rule and a text list's bullet. |
| `destinations.<name>.symbols.bullet` | [OneCharacter](#onecharacter) | A list's bullet where a list is written as text: `•` in Slack and `-` in plain text where unset. |
| `destinations.<name>.symbols.done` | [OneCharacter](#onecharacter) | A task done; `☑` where unset. |
| `destinations.<name>.symbols.rule` | [OneCharacter](#onecharacter) | A rule's character, drawn three wide; `─` where unset. |
| `destinations.<name>.symbols.todo` | [OneCharacter](#onecharacter) | A task to do; `☐` where unset. |
| `destinations.<name>.tenant` | string | The id of the Microsoft Entra tenant the bot's app is registered in, for a bot registered for one tenant; where unset, the cloud's own tenant for bots registered for every tenant: `botframework.com` in the public cloud. |
| `destinations.<name>.waits` | table | How long it waits before sending again where Teams answers that it is unwell or limited and names no wait. |
| `destinations.<name>.waits.limited` | [Duration](#duration) | After the platform answers that herald sends too often; the platform's own window where unset: "5m" on Mastodon, "5s" elsewhere. |
| `destinations.<name>.waits.outage` | [Duration](#duration) | After the platform answers that it is unwell; "5s" where unset. |
| `destinations.<name>.webhook` | table | The secret holding a Workflows webhook's URL: the URL of a workflow whose trigger is "When a Teams webhook request is received", set so that anyone may trigger it. It posts to the channel or chat its workflow names, and only posts. |
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
| `destinations.<name>.key` | table | The secret holding the actors' RSA private key, as PKCS#8 PEM, such as `herald activitypub keygen` writes; `[federation] actor_key` where unset. |
| `destinations.<name>.key.secret` | string | The secret's name. |
| `destinations.<name>.kind` | string |  |
| `destinations.<name>.lists` | array of tables | Shared block lists, each a file of Mastodon's domain block CSV pinned by its SHA-256, whose domains are blocked beside `block`'s; `[federation]`'s where unset. |
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

### `destinations.<name>` of kind `herald-push`

A peer's herald source each event is pushed to: an envelope proved by the instance's newest key, in a request signed by it.

| Setting | Type | What it sets |
|---------|------|--------------|
| `destinations.<name>` | table | A peer's herald source each event is pushed to: an envelope proved by the instance's newest key, in a request signed by it. |
| `destinations.<name>.audience` | string | What the run's ID token must be meant for with `credential = "id-token"`: the peer's URL where absent, and always on the peer's origin. A token whose `aud` names another is never sent, so no herald is handed a token meant for another, which it could present there. |
| `destinations.<name>.credential` | [PushCredential](#pushcredential) | What proves who pushes: "key", where absent, proves each envelope and signs each request with `[federation]`'s newest key; "id-token" presents the CI run's ID token, which an app of the peer's takes, and the peer proves each envelope itself. |
| `destinations.<name>.envelope` | [Size](#size) | The most bytes one envelope takes, its proof counted: "16KiB" where unset, and at most "64KiB". A longer event's body is cut to fit. |
| `destinations.<name>.id_token` | [IdTokenSource](#idtokensource) | Where the run's ID tokens come from with `credential = "id-token"`: "actions", where absent, asks GitHub Actions for a fresh one for each push, which needs the job's `id-token: write` permission; "variables" reads them from `id_tokens`, each presented for one push in turn, as GitLab CI's `id_tokens:` sets them. |
| `destinations.<name>.id_tokens` | array of tables | The run's variables holding its ID tokens with `id_token = "variables"`, each a token for one push: `[{ secret = "HERALD_ID_TOKEN" }]` where absent. A peer whose app takes each token once refuses a second push with one; give as many as the run pushes, each its own `id_tokens:` entry with the peer's URL as its `aud`. The pushes past them wait for a later run. |
| `destinations.<name>.id_tokens[].secret` | string | The secret's name. |
| `destinations.<name>.inbox` | string | The peer's herald source taking the pushes, whose inbox is `/herald/v1/inbox/<inbox>` under the peer's URL. |
| `destinations.<name>.kind` | string |  |
| `destinations.<name>.peer` | string | The peer of `[peers]` each event is pushed to. |
| `destinations.<name>.waits` | table | How long it waits before pushing again where the peer's inbox answers that it is unwell or limited and names no wait. |
| `destinations.<name>.waits.limited` | [Duration](#duration) | After the platform answers that herald sends too often; the platform's own window where unset: "5m" on Mastodon, "5s" elsewhere. |
| `destinations.<name>.waits.outage` | [Duration](#duration) | After the platform answers that it is unwell; "5s" where unset. |

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
| `federation.grants_ending` | [Duration](#duration) | How soon before a grant ends doctor names it: "7d" where absent, at most "90d" (ADR-0054). |
| `federation.keep_audit` | [Duration](#duration) | How long what each peer asked is kept, by UTC day: "90d" where absent, at most "365d" (ADR-0054). |
| `federation.keys` | array of tables | The secrets holding the instance's Ed25519 private keys, each as PKCS#8 PEM, such as `openssl genpkey -algorithm ed25519` writes, newest first: the first signs, and every one verifies. Rotate by writing a new key before the old one, and leave the old one out once peers know the new one. |
| `federation.keys[].secret` | string | The secret's name. |
| `federation.lists` | array of tables | Shared block lists, for an ActivityPub destination that names no `lists`. |
| `federation.lists[].file` | string | The file, as a path beside `herald.toml`. |
| `federation.lists[].sha256` | string | The SHA-256 of the file's text, in hex: a list that changes is not read until its new digest is pinned. |
| `federation.mode` | [Mode](#mode) | Whom herald's actors federate with, for an ActivityPub destination that names no `mode`: `open` or `allowlist`. |
| `federation.pages` | table | Where herald writes its feeds as a static signed log for any static host to serve, such as GitHub Pages, at the end of each run that changed one (ADR-0060). None where absent. |
| `federation.pages.api` | string | The API's base URL, for a GitHub Enterprise Server; GitHub's own API where absent. |
| `federation.pages.artifact` | string | A directory each writing of the log fills with every file of it, such as for actions/upload-pages-artifact to deploy. None where absent. |
| `federation.pages.branch` | string | The branch each writing of the log is committed to, one commit each, for GitHub Pages or any static host to serve; `herald-pages` where absent, unless `artifact` is named, which then takes its place. |
| `federation.pages.refresh` | [Duration](#duration) | How long a log no feed changed is left before it is written again under its next sequence, so its readers see it is current: "1d" where absent, at most "7d". |
| `federation.pages.repository` | string | The repository holding the branch, as `owner/name`; the run's own where absent. |
| `federation.pages.token` | table | The secret holding a token that may push to the branch; `GITHUB_TOKEN` where absent. |
| `federation.pages.token.secret` | string | The secret's name. |
| `federation.pages.url` | string | Where a static host serves the log, such as `https://owner.github.io/repository`: https://, `[federation] domain` as its host, and a path where the log is not served at the root. |
| `federation.refresh` | [Duration](#duration) | How long a peer may keep the instance's key set before it reads it again; "1d" where absent, at most "7d". |
| `federation.refused_nonces` | integer | How many requests of one peer refused past its rate have their nonces kept, counted afresh every 20 minutes, the longest a request lives, so none is taken later: 2400 where absent, its rate's default over that time, at most 9600. Past it the peer is held, and nothing of it is taken until every request it made unkept has expired (ADR-0054). |
| `federation.replay_rate` | [Rate](#rate) | How many requests replaying a nonce one peer already used are kept in its audit, counted apart from its own rate so a replay spends none of it: "120/m" where absent, at most "480/m"; past it herald answers 429 (ADR-0054). |
| `federation.revoked` | array of string | Peer keys that are never trusted again, each a Multikey such as `z6Mk…`, as `herald peer revoke` adds them: a peer pinning one does not load, and a new key set naming one is refused. |
| `federation.signed_for` | [Duration](#duration) | How long each response of herald's API, and each push it makes to a peer, stays signed, from when it is made: "5m" where absent, at most "20m". |
| `federation.stranger_rate` | [Rate](#rate) | How many requests to a source's inbox that no peer's key signs are kept in the audit: "120/m" where absent, at most "480/m"; the rest are only logged (ADR-0054). |

## `files`

The directories beside its own whose files the config may name: a template's, a member list's, a block list's or a CA file elsewhere stops the load (ADR-0092 decision 8).

| Setting | Type | What it sets |
|---------|------|--------------|
| `files.allowed` | array of string | The directories, each an absolute path, whose files a template's `file`, a source's `members.file`, a block list's `file` and `http.ca_file` may name beside the config's own directory; none where absent. Every link is followed before a file is read. |

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
| `kinds.<name>.attributes` | table | The attributes its events carry, by name, as `event.attributes.<name>` reads them. |
| `kinds.<name>.attributes.<name>.required` | boolean | Whether every event of the kind carries it. |
| `kinds.<name>.attributes.<name>.type` | [AttributeType](#attributetype) | What it holds. |
| `kinds.<name>.description` | string | What it is, in a sentence. |
| `kinds.<name>.links` | array of tables | The links that become its buttons, in order: each a name, labelled by its name, or a name and a label. |

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

## `log`

What herald's log writes, and the secrets kept out of everything herald writes.

| Setting | Type | What it sets |
|---------|------|--------------|
| `log.format` | [LogFormat](#logformat) | How herald's log writes each line: "text", as words, where absent; or "json", a JSON object a line holding its `time` (RFC 3339, UTC), its `level` and its `message`. `HERALD_LOG_FORMAT` takes its place, and a command's `--log-format` takes the place of both. |
| `log.level` | [Level](#level) | The least grave line herald's log writes: "debug", every line and what herald saw as it went, none carrying what a private repository holds; "info", every line but those, where absent; "warn", what herald left undone, passed over or refused, and what stopped or failed; or "error", what stopped or failed alone. `HERALD_LOG_LEVEL` takes its place, and a command's `--log-level` takes the place of both. |
| `log.scrub_from` | integer | The fewest characters a secret's value holds to be written as `«secret:NAME»` in everything herald writes: 8 where absent, as a shorter value would rewrite ordinary words. On GitHub Actions every secret is masked whatever its length. |
| `log.scrub_warn_past` | integer | How many secret values herald holds to scrub before its log says, once, that it holds more: 4096 where absent. Past it every value is still held and scrubbed until it can no longer be valid; raise it where a run sends that many secrets by design. |

## `markers`

The installation's marker key.

| Setting | Type | What it sets |
|---------|------|--------------|
| `markers.key` | table | The secret holding the marker key, such as `HERALD_MARKER_SECRET`. |
| `markers.key.secret` | string | The secret's name. |

## `merge_queues`

The merge queues, by name: each a base branch whose pull requests a writer queues the forge's merge queue or herald's own merges.

| Setting | Type | What it sets |
|---------|------|--------------|
| `merge_queues.<name>.applied_by` | [AppliedBy](#appliedby) | Where its steps are taken: "command", by `herald merge-queue --apply`; "server", by `herald server`, which then reads the write credential. "command" where absent. |
| `merge_queues.<name>.branch` | string | The base branch it merges into; one table per branch. |

### `merge_queues.<name>.enter`: A label of this name added to the pull request

| Setting | Type | What it sets |
|---------|------|--------------|
| `merge_queues.<name>.enter` | table | A label of this name added to the pull request. |
| `merge_queues.<name>.enter.label` | string |  |

### `merge_queues.<name>.enter`: A comment on the pull request whose whole body, trimmed, is this command

| Setting | Type | What it sets |
|---------|------|--------------|
| `merge_queues.<name>.enter` | table | A comment on the pull request whose whole body, trimmed, is this command. |
| `merge_queues.<name>.enter.comment` | string |  |

| Setting | Type | What it sets |
|---------|------|--------------|
| `merge_queues.<name>.least` | [Permission](#permission) | The least role the person who queues holds on the repository: "write" (GitHub write, GitLab Developer, Forgejo write, or above), "maintain" or "admin"; "write" where absent. |
| `merge_queues.<name>.may_fail` | array of string | The checks, by name, that may be red or missing on the branch; never one the forge requires. |
| `merge_queues.<name>.message` | [Message](#message) | The commit message: "forge", the forge's own for the method, or "pull", the pull request's title and body, with "merge" or "squash" alone; "forge" where absent. |
| `merge_queues.<name>.method` | [Method](#method) | How it merges: "merge", "squash", "rebase", "fast-forward", or "repository", the repository's one allowed method. A queue without it does not start. |
| `merge_queues.<name>.repository` | string | The repository it merges in, by full path, where the source is an owner; the source's own where absent. |
| `merge_queues.<name>.retries` | table | The checks whose failed job is re-run, at most `times` on one head, before the pull request leaves; none where absent, and none on Forgejo. |
| `merge_queues.<name>.retries.checks` | array of string | The checks, by name, whose failed job is re-run. |
| `merge_queues.<name>.retries.times` | integer | The most re-runs on one head. |
| `merge_queues.<name>.role_lost` | [RoleLost](#rolelost) | What becomes of a pull request herald handed to the forge's queue, or found already there, once the person whose gesture it is no longer holds `least` (a lower role, none, or an account since deleted), whether or not a cancel herald does not carry out stands: "take-out", taken out on herald's next applying run, which says why; "stay", left in the forge's queue and said so. Refused with `runs = "herald"`. "take-out" where absent. |
| `merge_queues.<name>.runs` | [MergeRuns](#mergeruns) | Who runs it: "forge" hands each pull request to the forge's merge queue or merge train; "herald" runs herald's own serial queue, also where the forge has one. "forge" where absent. |
| `merge_queues.<name>.signer` | string | The committer a GitHub Enterprise Server or GHE.com instance's web flow signs the commits it makes for, as its signature names it: the operator vouches that it is the instance's own identity, never a user's. With `updated = "signed"` it lets a gesture vouch for the head herald's own update made there; github.com's own, `noreply@github.com`, where absent there; herald's own queue on GitHub alone. |
| `merge_queues.<name>.skew` | [Duration](#duration) | How far apart the forge's stamps of a gesture and of a push may be, its services' clocks never quite agreeing: a push stamped up to this before a gesture, or before a GitLab approval, counts as after it, so the gesture vouches for no head that push may have brought; "5s" where absent. |
| `merge_queues.<name>.source` | string | The source whose repository it merges in: one of kind "github", "gitlab" or "forgejo". |
| `merge_queues.<name>.update_wait` | [Lasting](#lasting) | How long herald pauses in all, at most, for the head of a pull request it brought up to date to move, where the forge answered before it moved: herald reads the head right after its update, and again every 2s while the head has not moved and the pauses stay within this; "0s" reads it once. "10s" where absent, at most "1m"; herald's own queue alone, read with `updated = "signed"` alone. |
| `merge_queues.<name>.updated` | [MergeUpdated](#mergeupdated) | Whether a gesture vouches for the head herald's own update made from the head it vouched for: "signed", where the new head's parents are exactly that head and the base's head herald recorded, and the forge itself signed it; "never", the pull request leaves and a new gesture queues it; "approved", once one who holds `least` approved the new head. Where absent, "signed" where herald can prove the forge signed that head (github.com, or GitHub Enterprise Server and GHE.com with `signer` set) and "never" elsewhere; herald's own queue alone. |
| `merge_queues.<name>.vouches` | [Vouches](#vouches) | Which head a gesture vouches for: "gesture", the head at the gesture's time on the forge; "seen", the head herald first reads with the gesture; "approved", that head once one who holds `least` approved it on the forge. "gesture" where absent. |
| `merge_queues.<name>.wait` | [Duration](#duration) | How long it waits for a head's checks before the pull request leaves; "2h" where absent. |
| `merge_queues.<name>.write` | table | The credential its steps are made with, held apart from every one herald reads with: `{ token = { secret = "NAME" } }` or `{ app = "<github_apps name>" }`, the same as its source's `write` or another. The queue does not start without it. |
| `merge_queues.<name>.write.app` | string | The GitHub App the writes are made as, by the name of its `[github_apps.<name>]` table, in place of `token`: an App other than the one the source reads as, installed with the permissions its writes need alone. |
| `merge_queues.<name>.write.installation` | integer | The id of the App's installation tokens are minted for; looked up on the owner, or on the repository, where absent. |
| `merge_queues.<name>.write.token` | table | A token that may make the writes, other than the one the source reads with. |
| `merge_queues.<name>.write.token.secret` | string | The secret's name. |

## `models`

The models tasks ask, by name: each a decision model or a server speaking OpenAI's chat completions.

| Setting | Type | What it sets |
|---------|------|--------------|
| `models.<name>.access_key_id` | table | The secret holding the access key's id a "bedrock" model's requests are signed with where it names no key; `AWS_ACCESS_KEY_ID` where absent, as a Lambda's role provides it. |
| `models.<name>.access_key_id.secret` | string | The secret's name. |
| `models.<name>.base` | string | Its API's base, such as `http://127.0.0.1:11434/v1`; the preset's where absent, and required where it has none. A "bedrock" model's is its region's endpoint where absent. |
| `models.<name>.binding` | string | The Worker's binding a "workers-ai" model is reached through, such as "AI", in place of Workers AI's REST API; it takes neither a base nor a key. |
| `models.<name>.header` | string | The header its key goes in, such as "api-key"; `Authorization: Bearer` where absent and the preset names none. |
| `models.<name>.insecure` | boolean | Whether it may be reached over plain HTTP at an address that is not this machine's, which doctor names. |
| `models.<name>.key` | table | Its key, where it takes one; a "bedrock" model's is a Bedrock API key, sent in place of a signature. |
| `models.<name>.key.secret` | string | The secret's name. |
| `models.<name>.kind` | string | How it is spoken to: "typesafe" for TypeSafe's Jev, "openai-compatible" for any server speaking OpenAI's chat completions, or a preset of it, such as "ollama" or "mistral", "anthropic" for Anthropic's Messages API, "gemini" for Gemini's, "bedrock" for Amazon Bedrock's Converse API, or "spin" for a model Spin runs; "workers-ai" with a binding is a model Workers AI runs. |
| `models.<name>.model` | string | The model's id, pinned rather than an alias such as `jev-latest`. |
| `models.<name>.most` | integer | The most bytes of its answer read: 65536 where absent. |
| `models.<name>.private` | boolean | Whether it may be given a private or internal repository's data, which no model is, on this machine or elsewhere, unless it says `private = true`. |
| `models.<name>.region` | string | The region a "bedrock" model is reached in, such as "eu-west-1", in any partition, such as "us-gov-west-1" or "cn-north-1". |
| `models.<name>.secret_access_key` | table | The secret holding the secret access key; `AWS_SECRET_ACCESS_KEY` where absent. |
| `models.<name>.secret_access_key.secret` | string | The secret's name. |
| `models.<name>.session_token` | table | The secret holding a session token, sent where it is set; `AWS_SESSION_TOKEN` where absent. |
| `models.<name>.session_token.secret` | string | The secret's name. |
| `models.<name>.tokens` | integer | The most tokens it answers within, where its kind takes a cap, as every kind but "typesafe" and "openai-compatible" does: 4096 where absent. |

## `peers`

The other heralds this one trusts, by name.

| Setting | Type | What it sets |
|---------|------|--------------|
| `peers.<name>.grants` | array of tables | What the peer may do with this herald's resources, each also granted on the resource itself, such as `[{ resource = "sources.upstream", actions = ["push"] }]`. None where absent. |
| `peers.<name>.grants[].actions` | array of [Action](#action) | What the peer may do with it, such as `["push"]`. |
| `peers.<name>.grants[].namespaces` | array of string | The identities the peer may give as they are, each the start of identities up to a '/'. None where absent. |
| `peers.<name>.grants[].resource` | string | The resource, such as "sources.upstream", a `herald` source. |
| `peers.<name>.grants[].until` | string | When the grant ends, in RFC 3339 such as "2027-01-01T00:00:00Z" or as a TOML datetime with its offset; never where absent. |
| `peers.<name>.hops` | integer | The most instances an object from the peer may have passed through: 2 where absent, at most 8. |
| `peers.<name>.keys` | array of string | The peer's Ed25519 keys, each a Multikey as its discovery document lists it, such as `z6Mk…`, pinned by `herald peer add` once the operator confirms them. A key is never taken from the network unless a pinned key signed it. |
| `peers.<name>.most` | [Size](#size) | The most bytes of a body the peer sends that are read: "2MiB" where absent, at most "8MiB". A longer one is refused before it is parsed. |
| `peers.<name>.private` | boolean | Whether private events may go to the peer; false where absent. |
| `peers.<name>.rate` | [Rate](#rate) | How many requests the peer may make of this herald: "120/m" where absent, at most "480/m". |
| `peers.<name>.rotation` | [Rotation](#rotation) | Whether a new key set the peer signs with a pinned key replaces the pinned keys: `manual` (the operator pins each new key) where absent, or `signed`. |
| `peers.<name>.url` | string | Where the peer is served, such as `https://herald.example`: `https://`, its domain, and the path its endpoints are under where they are not at the root. Its discovery document is `/.well-known/herald` under it. |

## `presets`

The presets the config adopts, by name: each expanded into the tables it writes, each of them named `<name>-<table>` (ADR-0092).

### `presets.<name>`: `herald:announce-releases`: Each release of its sources, announced once to its destination with its notes

| Setting | Type | What it sets |
|---------|------|--------------|
| `presets.<name>` | table | `herald:announce-releases`: Each release of its sources, announced once to its destination with its notes. |
| `presets.<name>.body` | string | The template of each announcement's body. |
| `presets.<name>.destination` | string | The destination its routes announce to, as `[destinations]` names it. |
| `presets.<name>.prereleases` | [Prereleases](#prereleases) | Which pre-releases it announces: `include`, `exclude` or `only`. |
| `presets.<name>.sources` | array of string | The sources whose events its routes take, as `[sources]` names them. |
| `presets.<name>.title` | string | The template of each announcement's title. |
| `presets.<name>.use` | string | The preset it adopts. |

### `presets.<name>`: `herald:review-reminders`: Each open pull request waiting on review past each of its stages, nudged at 09:00 on weekdays to its destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `presets.<name>` | table | `herald:review-reminders`: Each open pull request waiting on review past each of its stages, nudged at 09:00 on weekdays to its destination. |
| `presets.<name>.at` | string | The wall-clock time, as "HH:MM", it nudges at. |
| `presets.<name>.days` | array of [Day](#day) | The days it nudges on. |
| `presets.<name>.destination` | string | The destination its routes announce to, as `[destinations]` names it. |
| `presets.<name>.sources` | array of string | The sources whose open pull requests it watches, as `[sources]` names them; every forge source where not given. |
| `presets.<name>.stages` | array of [Duration](#duration) | How long a pull request waits on review, since anyone last acted on it, before each nudge. |
| `presets.<name>.use` | string | The preset it adopts. |
| `presets.<name>.zone` | string | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam". |

### `presets.<name>`: `herald:merge-queue-small-team`: A merge queue on one base branch, which a writer enters a pull request into by a label or a comment

| Setting | Type | What it sets |
|---------|------|--------------|
| `presets.<name>` | table | `herald:merge-queue-small-team`: A merge queue on one base branch, which a writer enters a pull request into by a label or a comment. |
| `presets.<name>.branch` | string | The base branch it merges into, such as `main`. |

### `presets.<name>.enter`: A label of this name added to the pull request

| Setting | Type | What it sets |
|---------|------|--------------|
| `presets.<name>.enter` | table | A label of this name added to the pull request. |
| `presets.<name>.enter.label` | string |  |

### `presets.<name>.enter`: A comment on the pull request whose whole body, trimmed, is this command

| Setting | Type | What it sets |
|---------|------|--------------|
| `presets.<name>.enter` | table | A comment on the pull request whose whole body, trimmed, is this command. |
| `presets.<name>.enter.comment` | string |  |

| Setting | Type | What it sets |
|---------|------|--------------|
| `presets.<name>.method` | [Method](#method) | How it merges: `merge`, `squash`, `rebase`, `fast-forward`, or `repository`, the repository's one allowed method. |
| `presets.<name>.runs` | [MergeRuns](#mergeruns) | Who runs it: `forge`, the forge's own merge queue or merge train, where not given; or `herald`, herald's own serial queue. |
| `presets.<name>.source` | string | The source whose repository it merges in, of kind `github`, `gitlab` or `forgejo`, as `[sources]` names it. |
| `presets.<name>.use` | string | The preset it adopts. |
| `presets.<name>.write` | table | The credential its steps are made with, held apart from every one herald reads with: `{ token = { secret = "NAME" } }` or `{ app = "<github_apps name>" }`. |
| `presets.<name>.write.app` | string | The GitHub App the writes are made as, by the name of its `[github_apps.<name>]` table, in place of `token`: an App other than the one the source reads as, installed with the permissions its writes need alone. |
| `presets.<name>.write.installation` | integer | The id of the App's installation tokens are minted for; looked up on the owner, or on the repository, where absent. |
| `presets.<name>.write.token` | table | A token that may make the writes, other than the one the source reads with. |
| `presets.<name>.write.token.secret` | string | The secret's name. |

### `presets.<name>`: `herald:security-alerts-private`: Each security alert of its sources, announced to its destination, which its route states is private

| Setting | Type | What it sets |
|---------|------|--------------|
| `presets.<name>` | table | `herald:security-alerts-private`: Each security alert of its sources, announced to its destination, which its route states is private. |
| `presets.<name>.body` | string | The template of each announcement's body. |
| `presets.<name>.destination` | string | The destination its routes announce to, as `[destinations]` names it. |
| `presets.<name>.sources` | array of string | The sources whose events its routes take, as `[sources]` names them. |
| `presets.<name>.title` | string | The template of each announcement's title. |
| `presets.<name>.use` | string | The preset it adopts. |

### `presets.<name>`: A local preset: a file inside herald.toml's directory, of version 1, writing routes, templates, views, reports and rules only

| Setting | Type | What it sets |
|---------|------|--------------|
| `presets.<name>` | table | A local preset: a file inside herald.toml's directory, of version 1, writing routes, templates, views, reports and rules only. |
| `presets.<name>.use` | string | The file, named from herald.toml's directory as `./<path>`. |

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
| `routes[].area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
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
| `routes[].living` | boolean | Whether it keeps one post of each report it takes, edited each window where the destination edits and posted anew each window where it cannot; it names only reports among its sources. |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `routes[].match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].subscribe` | table | The opt-in role members subscribe to, where the route offers one. |
| `routes[].subscribe.button` | boolean | Whether each announcement carries a button that subscribes, or unsubscribes a member already subscribed. |
| `routes[].subscribe.role` | string | The opt-in role `subscribe` gives a member and `unsubscribe` takes away, by id. The route mentions it with its announcements. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `routes[].templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `routes[].templates.<name>` | string |  |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].viewers` | array of string | The ids of the roles, and of any member a permission overwrite of their own lets in, expected to view the channel: an advisories section is sent there only where every role but `@everyone` that may view it is listed here, an Administrator role and the bot's own role included, and no member it does not list is let in by an overwrite; the server's owner sees every channel and is not checked (ADR-0067). |
| `routes[].vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route to a Mastodon destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
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
| `routes[].living` | boolean | Whether it keeps one post of each report it takes, edited each window where the destination edits and posted anew each window where it cannot; it names only reports among its sources. |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `routes[].match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `routes[].templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `routes[].templates.<name>` | string |  |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].title` | integer | The longest title, a status's first line, in characters; 100 where unset. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].visibility` | [Visibility](#visibility) | Who sees each status. |
| `routes[].vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route to a Matrix destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
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
| `routes[].living` | boolean | Whether it keeps one post of each report it takes, edited each window where the destination edits and posted anew each window where it cannot; it names only reports among its sources. |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `routes[].match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].room` | string | The room's id, such as `!abcdefg:matrix.org`, which the bot has joined. |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `routes[].templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `routes[].templates.<name>` | string |  |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route to a Slack destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
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
| `routes[].living` | boolean | Whether it keeps one post of each report it takes, edited each window where the destination edits and posted anew each window where it cannot; it names only reports among its sources. |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `routes[].match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].subscribe` | table | How the route offers subscribing, where it mentions its subscribers. |
| `routes[].subscribe.button` | boolean | Whether each announcement carries a button that subscribes, or unsubscribes a member already subscribed. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `routes[].templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `routes[].templates.<name>` | string |  |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route to a Microsoft Teams destination

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
| `routes[].conversation` | string | The id of the channel or chat the bot posts to, such as `19:…@thread.tacv2`, where its app is installed. A bot needs one; a webhook posts where its workflow names. |
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
| `routes[].living` | boolean | Whether it keeps one post of each report it takes, edited each window where the destination edits and posted anew each window where it cannot; it names only reports among its sources. |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `routes[].match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `routes[].templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `routes[].templates.<name>` | string |  |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].threads` | boolean | Whether the bot posts each overflow part as a reply in its post's thread, through Microsoft's `replyChains`, which Microsoft marks as a preview; each part is posted on its own where unset. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route announcing as one of herald's own ActivityPub actors

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].actor` | string | The actor the route announces as, such as `php-mutation-gate`, which is followed as `@php-mutation-gate@<domain>`; or a template naming each project's own actor from `event.project`, such as `{{ event.project }}`. A project whose name is no actor's name is not announced on the route. |
| `routes[].area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
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
| `routes[].living` | boolean | Whether it keeps one post of each report it takes, edited each window where the destination edits and posted anew each window where it cannot; it names only reports among its sources. |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `routes[].match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `routes[].templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `routes[].templates.<name>` | string |  |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].visibility` | [Audience](#audience) | Who sees each Note on the timelines. |
| `routes[].vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route publishing its events to a feed of herald's own

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
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
| `routes[].living` | boolean | Whether it keeps one post of each report it takes, edited each window where the destination edits and posted anew each window where it cannot; it names only reports among its sources. |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `routes[].match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `routes[].templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `routes[].templates.<name>` | string |  |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

### `routes[]`: A route pushing its events to a peer's herald source

| Setting | Type | What it sets |
|---------|------|--------------|
| `routes[].area_paths` | [AreaPaths](#areapaths) | Where the paths a pull request's head changes are read, which the areas its `match` narrows by are those of: `kept`, from the pull request as herald keeps it where its head is the event's, no request more; or `delivery`, from the forge as the event is delivered, one request more for each pull request event (ADR-0072). |
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
| `routes[].living` | boolean | Whether it keeps one post of each report it takes, edited each window where the destination edits and posted anew each window where it cannot; it names only reports among its sources. |
| `routes[].match` | table | What of an event it narrows its sources' events by, an event taken only where every key holds; every event its other keys take where absent. |
| `routes[].match.areas` | array of string | The areas, as globs ignoring case, any one of which the pull request an event is of falls in, each matching an area one of the route's sources names; an event whose areas are not known is not taken, and under `not` is never taken (ADR-0072). |
| `routes[].match.attributes` | table | The attributes of the event's kind, every one of which it carries with one of the values given for it, as text and ignoring case, such as `conclusion = ["failure", "timed_out"]`; any value where none is given. |
| `routes[].match.attributes.<name>` | array of string |  |
| `routes[].match.forges` | array of string | The forges and registries taken, as a source's `kind` names them. |
| `routes[].match.hosts` | array of string | The hosts taken, such as `github.com` or `ghe.example`. |
| `routes[].match.owners` | array of string | The owners taken: accounts, organizations, groups, or a registry's scopes, vendors and namespaces, as globs, ignoring case. |
| `routes[].match.projects` | array of string | The projects taken, as globs, ignoring case. |
| `routes[].match.properties` | table | The custom properties, every one of which a repository holds with one of the values given for it; any value where none is given. |
| `routes[].match.properties.<name>` | array of string |  |
| `routes[].match.repositories` | array of string | The repositories or packages taken by their full path, as globs, ignoring case, such as `"*/plugin-*"`. |
| `routes[].match.topics` | array of string | The topics, any one of which a repository holds. |
| `routes[].mentions` | table | Who it mentions. |
| `routes[].mentions.roles` | array of string | The roles it pings, by the destination's ids for them. |
| `routes[].mentions.subscribers` | boolean | Whether it mentions the subscribers herald keeps. |
| `routes[].pin` | [Pin](#pin) | Whether to pin each announcement in place of the route's earlier pin, and how the earlier pin is found: `true` finds it by its marker, as "markers" does, and `false` pins nothing. A living route pins its post where absent and the destination pins. |
| `routes[].prereleases` | [Prereleases](#prereleases) | Which pre-releases it takes. |
| `routes[].private` | boolean | Whether its place is private, so it may take a private or internal repository's events; a place is public where absent, and a config sending such events to it does not load (`[doctor] private_to_public`). A route taking a private kind, such as `alert`, says it either way: `false` sends them to a public place on purpose (`[doctor] private_kinds`). |
| `routes[].sources` | array of string | The sources it takes events from. |
| `routes[].template` | string | The template it renders with; none on a route to a herald feed or push, which carries each event as an envelope. |
| `routes[].templates` | table | The template it renders with in place of `template` where its destination's dialect is one of these, each keyed by the kind of destination that writes the dialect, such as `{ mastodon = "release-short" }`. Each is Markdown, converted to the dialect as `template` is; `template` where its destination's dialect has none. |
| `routes[].templates.<name>` | string |  |
| `routes[].thread` | table | The thread to open for each announcement. |
| `routes[].thread.archive_after` | [Duration](#duration) | How long after the announcement the thread is archived. |
| `routes[].thread.lock_after` | [Duration](#duration) | How long after the announcement the thread is locked, and archived. |
| `routes[].thread.name` | string | The template of the thread's name. |
| `routes[].versions` | string | The versions it takes, as a pattern in which `*` stands for any run of characters and `?` for any one, such as `"*.*.*"`, which leaves out a Docker Hub tag such as `latest`; every version where absent. |
| `routes[].vouch_unchecked` | boolean | Whether the operator vouches that its place is private where herald cannot check who may read it, as for a Discord or Slack route reached by a webhook: with `private = true` it is then shown a view's advisories on the operator's word, which doctor lists. Refused on a route herald checks before each run's first advisory send, and on one that posts where anyone may read. False where absent. |
| `routes[].withdraw` | [Withdraw](#withdraw) | What it does with an announcement whose Release the forge deletes: keeps its post (`keep`), or deletes every message herald recorded of it where the destination can (`delete`). |

## `rules`

The rules routes name among their sources, by name: each a view watched once a window, nudging each pull request that waited past a stage.

| Setting | Type | What it sets |
|---------|------|--------------|
| `rules.<name>.at` | string | The wall-clock time a day's first window begins at, as "HH:MM": "09:00" where absent. |
| `rules.<name>.days` | array of [Day](#day) | The days windows begin on, every day where empty. |
| `rules.<name>.dequeued` | [RuleDequeued](#ruledequeued) | Whether it alerts on each pull request of its view that left its forge's merge queue or herald's own, once each time one left: `true` for the reasons that are a failure of the pull request ("checks-failed", "checks-timed-out", "conflict", "invalid", "protections", "cannot-update", "head-moved", "unrecorded", "unmergeable", "merge-refused", "unread"), or the reasons it names, such as `["checks-failed", "removed"]`; none where absent. Why a pull request left its forge's queue is known where its source is read kept, from GitHub's `pull_request` `dequeued` webhook (ADR-0078); why it left herald's own, from the record herald's own queue keeps in the store (ADR-0087). |
| `rules.<name>.every` | [Duration](#duration) | How long each window lasts within a day, from "1m" to "1d": "1d" where absent. |
| `rules.<name>.flaky` | table | Whether it plans a re-run of each flaky check, as `{ rerun = true, mode = "plan" }`: each plan names its evidence and the exact call, announced once a run and head to the rule's private routes. A rule only plans; a re-run is made by an operator signed in through the forge, never by a rule on its own (ADR-0065). |
| `rules.<name>.flaky.mode` | [RerunMode](#rerunmode) | "plan", the only mode a rule takes: where absent too. |
| `rules.<name>.flaky.rerun` | boolean | Whether it plans a re-run of each flaky check. |
| `rules.<name>.flaky.runs` | integer | The most failed runs on one head a plan reads for one check, the newest first, from 1 to 100: 5 where absent. A plan takes one request for them and, on GitHub, one more for each. |
| `rules.<name>.most` | integer | The most pull requests it nudges a window, the longest waiting first, and the most re-runs it plans: 20 where absent; the rest wait for the next window. |
| `rules.<name>.release` | table | What it alerts on of its view's release, as `{ ready = true, late = 7 }`: once the release turns ready, and once it is still held back `late` days before the due day of the milestone due first, from 0 to 365; each audience of its routes judged and alerted as its board tells it, a public route never told of a private repository. Neither where absent (ADR-0085). |
| `rules.<name>.release.late` | integer | How many days before the due day of the milestone due first it alerts on the release still held back, once a milestone and due day; it needs the release's header or issue rows to read the milestones. None where absent. |
| `rules.<name>.release.ready` | boolean | Whether it alerts once the release turns ready; false where absent. |
| `rules.<name>.stages` | array of [Duration](#duration) | How long an open pull request waits, since anyone last acted on it, before each nudge, such as `["2d", "7d"]`: each fires once a spell of waiting. |
| `rules.<name>.view` | string | The view it shows: "queue", every source's open pull requests, where absent. |
| `rules.<name>.zone` | string | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam": "UTC" where absent. |

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
| `server.keys.document` | [Size](#size) | The most of a key's or an actor's document read; "256KiB" where absent, and at most that: a setting may only lower it. |
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
| `sources.<name>.areas` | [Zones](#zones) | The areas of its repositories, each a name and the path globs, as `include` writes them, any of which a changed path falls in it by, such as `{ web = ["web/**", "packages/ui/**"] }`: at most `most_areas` areas and 1000 globs, each name and glob at most 128 and 256 characters, a path over 1024 falling in none. A view reading the source reads the paths each pull request's head changes, as one predicting conflicts does, and each row names its areas; none where absent (ADR-0071). Or "codeowners": each owner the repository's CODEOWNERS file names for a changed path, a team, a group, a role or a login, never an email, and each GitLab section, is an area (ADR-0073). |
| `sources.<name>.backfill` | integer | How many of the newest releases, tags and package versions the first poll announces; none where absent (ADR-0003). For an owner, it applies to the members the first listing finds; a member found later announces only what it publishes after it was found. |
| `sources.<name>.codeowners` | array of string | Where a repository's CODEOWNERS file is looked for, in order, the first found read: at most 8 paths in the repository; where absent, the forge's own places in its own order: GitHub `.github/`, the root, `docs/`; GitLab the root, `docs/`, `.gitlab/`; Forgejo the root, `docs/`, `.gitea/`, `.forgejo/` (ADR-0073). |
| `sources.<name>.codeowners_ref` | [CodeownersRef](#codeownersref) | The branch a repository's CODEOWNERS file is read at: "base", the branch each pull request merges into, or "default", the repository's default branch; where absent, the one the forge applies: "base" on GitHub and GitLab, "default" on Forgejo (Gitea applies "base": set it there) (ADR-0073). |
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
| `sources.<name>.milestones` | table | Which milestones are announced where the signals take "milestone": what they did, their titles, and whether their counts and a GitLab group's milestones are read; every milestone closed where absent. |
| `sources.<name>.milestones.actions` | array of [MilestoneAction](#milestoneaction) | What a milestone did that is announced: `opened`, `closed`, `reopened`, `deleted`, `overdue` (found only by a poll); `closed` where absent. |
| `sources.<name>.milestones.counts` | boolean | Whether a milestone names how many of its issues are open and closed, as its forge counts them; true where absent. On GitLab each announced milestone costs one request more. |
| `sources.<name>.milestones.group` | boolean | Whether the milestones of the GitLab group holding the project are read beside the project's own; false where absent. |
| `sources.<name>.milestones.most` | integer | The most open milestones of a repository a poll follows, in the order its forge lists them; 100 where absent, at most 1000. |
| `sources.<name>.milestones.titles` | array of string | The titles, as globs as `include` writes them, one of which a milestone announced matches; every milestone where empty. |
| `sources.<name>.most_areas` | integer | The most areas the source reads a repository's table or CODEOWNERS file naming: a table naming more is refused as the config is read, a file naming more is refused whole as it is read, and doctor says so; 200 where absent, and at most 2000, since each area is a group of a board and is matched against every changed path (ADR-0074). |
| `sources.<name>.most_codeowners` | [Size](#size) | The most of a CODEOWNERS file downloaded and read, a longer file refused unread; "128KiB" where absent, and at most "3MiB", GitHub's own limit (ADR-0073, ADR-0074). |
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
| `sources.<name>.properties` | table | Custom properties, every one of which a member holds with one of the values given, such as `{ team = "media", tier = ["core", "edge"] }`. |
| `sources.<name>.properties.<name>` | [Values](#values) | A property's values: one, or any of several. |
| `sources.<name>.pulls` | table | Which pull requests are announced where the signals take "pull-request": what they did, whether drafts are, and their labels and authors; every pull request opened, ready for review or merged where absent. |
| `sources.<name>.pulls.actions` | array of [PullAction](#pullaction) | What a pull request did that is announced: `opened`, `ready_for_review`, `reopened`, `merged`, `closed`; `["opened", "ready_for_review", "merged"]` where absent. |
| `sources.<name>.pulls.authors` | array of string | The authors, as globs, whose pull requests are announced, such as `dependabot[bot]`; every author where empty. |
| `sources.<name>.pulls.drafts` | boolean | Whether a draft's opening is announced; false where absent, so a draft is announced once it is ready for review. |
| `sources.<name>.pulls.labels` | array of string | The labels, as globs as `include` writes them, any one of which a pull request announced carries; every pull request where empty. |
| `sources.<name>.registry` | string | The host of the container registry an image is named on, as in `registry.example:5000`: where absent, ghcr.io beside GitHub's API, `containers.<host>` beside a GitHub Enterprise Server's or a GHE.com tenant's, each image's own on GitLab, and the instance's on Forgejo. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.repositories` | table | What the source makes of the repositories each glob names, as `include` writes them; where several name one repository, the narrowest that sets a key gives it. |
| `sources.<name>.repositories.<name>.areas` | [Zones](#zones) | Their areas, as the source's `areas` writes them, in place of the source's; the source's where absent (ADR-0071, ADR-0073). |
| `sources.<name>.repositories.<name>.codeowners` | array of string | Where their CODEOWNERS file is looked for, as the source's `codeowners` writes it; the source's where absent (ADR-0073). |
| `sources.<name>.repositories.<name>.codeowners_ref` | [CodeownersRef](#codeownersref) | The branch their CODEOWNERS file is read at, as the source's `codeowners_ref` writes it; the source's where absent (ADR-0073). |
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
| `sources.<name>.sign_in` | table | How an operator signs in at the forge to apply a re-run, by its OAuth device flow; herald reads their permission on the repository with it right before each call (ADR-0068). |
| `sources.<name>.sign_in.client_id` | string | The client id of the GitHub App, OAuth application or GitLab application with the device flow turned on; never that of the App a `write` names. A private GitHub repository needs a GitHub App's client id: its user token holds that App's read permissions alone. A GitHub App must be installed on the repository's account and hold no permission but read, its account permissions among them. |
| `sources.<name>.sign_in.least` | [Permission](#permission) | The least the operator must be allowed on the repository: "write" (GitHub's write role, GitLab's Developer role, or above), "maintain" or "admin"; "write" where absent. |
| `sources.<name>.sign_in.scopes` | array of string | The scopes asked for, each one that only reads: on GitHub `read:user` or `read:org`, none where absent; on GitLab `read_api`, `read_user`, `openid`, `profile` or `email`, `["read_api"]` where absent. The operator's token only reads who they are and what they may do on the repository; a scope that writes does not load. |
| `sources.<name>.sign_in.web` | string | The web root the device flow is asked at; where absent, the API's: `https://github.com` for `https://api.github.com`, a GHE.com tenant's `https://acme.ghe.com` for `https://api.acme.ghe.com`, and a GitHub Enterprise Server's or a self-managed GitLab's host for an API ending in `/api/v3` or `/api/v4`. |
| `sources.<name>.signals` | array of [Signal](#signal) | What the source announces of each repository: its published Releases, its version tags, the versions of its packages, or several; `["release"]` where absent. A tag and a Release of it share one identity, so each route announces it once. |
| `sources.<name>.subgroups` | boolean | Whether a GitLab group's members include the projects of its subgroups; true where absent. |
| `sources.<name>.tables` | array of string | The arrays of the member file that list repositories, such as `["repo", "ungoverned"]`; the file's own array where absent, which only JSON can hold. |
| `sources.<name>.tag_window` | [Duration](#duration) | How long a tag with no Release waits for one, counted from when herald first sees the tag; "1h" where absent, and at most "7d". |
| `sources.<name>.teams` | [TeamsFrom](#teamsfrom) | Where the teams a view's load counts come from: the config's `[teams.<name>]` alone ("config"), or also the forge's own teams of the organization or group owning the source's repositories ("forge"), a team the config names keeping the config's members. Each round a view showing its load reads the source, it reads at most 100 teams, each with at most 500 members, by one request a page; the members' logins are held for the round alone, never shown or kept, and a forge team's name and figures reach only a route that says `private = true` and `herald queue` with `--private` or `[cli] private`. GitHub's teams are each `org/team`, needing Members: read as a GitHub App or fine-grained token, or `read:org` as a classic token, on GitHub Enterprise Server and GHE.com too; GitLab's are the group and each group under it, by full path, needing `read_api`, self-managed included; Forgejo's and Gitea's are each `org/team`, needing `read:organization`. A forge that refuses folds the load as "config" does, and `herald doctor --online` names the permission; "config" where absent (ADR-0077). |
| `sources.<name>.template` | boolean | Whether template repositories are taken. |
| `sources.<name>.token` | table | A token that reaches a private repository or raises the API's rate limit, such as the run's `GITHUB_TOKEN`. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.topics` | array of string | Topics, any one of which a member holds; any where absent. |
| `sources.<name>.transport` | [Transport](#transport) | How an owner's members are polled; "auto" where absent. GitHub offers auto, graphql, rest and events; GitLab auto, graphql and rest; Forgejo auto, rest and feed. |
| `sources.<name>.update` | [Update](#update) | How a merge queue brings a pull request's branch up to date with its base through the forge: "merge" merges the base in, keeping the author's commits and signatures; "rebase" rebases the branch. GitLab offers a rebase alone; "rebase" there and "merge" elsewhere where absent (ADR-0087). |
| `sources.<name>.visibility` | array of [RepositoryVisibility](#repositoryvisibility) | The visibilities taken; only public repositories where absent. A repository whose visibility the forge does not name is private. A route that takes a private or internal member is refused unless it is `private` (`[doctor] private_to_public`). |
| `sources.<name>.webhook` | table | The secret the forge signs its webhook deliveries to `herald server` with: GitHub's and Forgejo's webhook secret, or GitLab's signing token (ADR-0007). An owner source takes its repositories' and its organization's deliveries, and announces one only where the owner's last membership record holds the repository when its work runs. |
| `sources.<name>.webhook.secret` | string | The secret's name. |
| `sources.<name>.write` | table | The credential an operator's `herald rerun --apply` re-runs a planned job with, apart from `token` and `app`: never read by a poll, a report or the server (ADR-0068). A stolen write credential re-runs without herald, so give it nothing but re-running. |
| `sources.<name>.write.app` | string | The GitHub App the writes are made as, by the name of its `[github_apps.<name>]` table, in place of `token`: an App other than the one the source reads as, installed with the permissions its writes need alone. |
| `sources.<name>.write.installation` | integer | The id of the App's installation tokens are minted for; looked up on the owner, or on the repository, where absent. |
| `sources.<name>.write.token` | table | A token that may make the writes, other than the one the source reads with. |
| `sources.<name>.write.token.secret` | string | The secret's name. |

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
| `sources.<name>.areas` | [Zones](#zones) | The areas of its repositories, each a name and the path globs, as `include` writes them, any of which a changed path falls in it by, such as `{ web = ["web/**", "packages/ui/**"] }`: at most `most_areas` areas and 1000 globs, each name and glob at most 128 and 256 characters, a path over 1024 falling in none. A view reading the source reads the paths each pull request's head changes, as one predicting conflicts does, and each row names its areas; none where absent (ADR-0071). Or "codeowners": each owner the repository's CODEOWNERS file names for a changed path, a team, a group, a role or a login, never an email, and each GitLab section, is an area (ADR-0073). |
| `sources.<name>.backfill` | integer | How many of the newest releases, tags and package versions the first poll announces; none where absent (ADR-0003). For an owner, it applies to the members the first listing finds; a member found later announces only what it publishes after it was found. |
| `sources.<name>.codeowners` | array of string | Where a repository's CODEOWNERS file is looked for, in order, the first found read: at most 8 paths in the repository; where absent, the forge's own places in its own order: GitHub `.github/`, the root, `docs/`; GitLab the root, `docs/`, `.gitlab/`; Forgejo the root, `docs/`, `.gitea/`, `.forgejo/` (ADR-0073). |
| `sources.<name>.codeowners_ref` | [CodeownersRef](#codeownersref) | The branch a repository's CODEOWNERS file is read at: "base", the branch each pull request merges into, or "default", the repository's default branch; where absent, the one the forge applies: "base" on GitHub and GitLab, "default" on Forgejo (Gitea applies "base": set it there) (ADR-0073). |
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
| `sources.<name>.milestones` | table | Which milestones are announced where the signals take "milestone": what they did, their titles, and whether their counts and a GitLab group's milestones are read; every milestone closed where absent. |
| `sources.<name>.milestones.actions` | array of [MilestoneAction](#milestoneaction) | What a milestone did that is announced: `opened`, `closed`, `reopened`, `deleted`, `overdue` (found only by a poll); `closed` where absent. |
| `sources.<name>.milestones.counts` | boolean | Whether a milestone names how many of its issues are open and closed, as its forge counts them; true where absent. On GitLab each announced milestone costs one request more. |
| `sources.<name>.milestones.group` | boolean | Whether the milestones of the GitLab group holding the project are read beside the project's own; false where absent. |
| `sources.<name>.milestones.most` | integer | The most open milestones of a repository a poll follows, in the order its forge lists them; 100 where absent, at most 1000. |
| `sources.<name>.milestones.titles` | array of string | The titles, as globs as `include` writes them, one of which a milestone announced matches; every milestone where empty. |
| `sources.<name>.most_areas` | integer | The most areas the source reads a repository's table or CODEOWNERS file naming: a table naming more is refused as the config is read, a file naming more is refused whole as it is read, and doctor says so; 200 where absent, and at most 2000, since each area is a group of a board and is matched against every changed path (ADR-0074). |
| `sources.<name>.most_codeowners` | [Size](#size) | The most of a CODEOWNERS file downloaded and read, a longer file refused unread; "128KiB" where absent, and at most "3MiB", GitHub's own limit (ADR-0073, ADR-0074). |
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
| `sources.<name>.properties` | table | Custom properties, every one of which a member holds with one of the values given, such as `{ team = "media", tier = ["core", "edge"] }`. |
| `sources.<name>.properties.<name>` | [Values](#values) | A property's values: one, or any of several. |
| `sources.<name>.pulls` | table | Which pull requests are announced where the signals take "pull-request": what they did, whether drafts are, and their labels and authors; every pull request opened, ready for review or merged where absent. |
| `sources.<name>.pulls.actions` | array of [PullAction](#pullaction) | What a pull request did that is announced: `opened`, `ready_for_review`, `reopened`, `merged`, `closed`; `["opened", "ready_for_review", "merged"]` where absent. |
| `sources.<name>.pulls.authors` | array of string | The authors, as globs, whose pull requests are announced, such as `dependabot[bot]`; every author where empty. |
| `sources.<name>.pulls.drafts` | boolean | Whether a draft's opening is announced; false where absent, so a draft is announced once it is ready for review. |
| `sources.<name>.pulls.labels` | array of string | The labels, as globs as `include` writes them, any one of which a pull request announced carries; every pull request where empty. |
| `sources.<name>.registry` | string | The host of the container registry an image is named on, as in `registry.example:5000`: where absent, ghcr.io beside GitHub's API, `containers.<host>` beside a GitHub Enterprise Server's or a GHE.com tenant's, each image's own on GitLab, and the instance's on Forgejo. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.repositories` | table | What the source makes of the repositories each glob names, as `include` writes them; where several name one repository, the narrowest that sets a key gives it. |
| `sources.<name>.repositories.<name>.areas` | [Zones](#zones) | Their areas, as the source's `areas` writes them, in place of the source's; the source's where absent (ADR-0071, ADR-0073). |
| `sources.<name>.repositories.<name>.codeowners` | array of string | Where their CODEOWNERS file is looked for, as the source's `codeowners` writes it; the source's where absent (ADR-0073). |
| `sources.<name>.repositories.<name>.codeowners_ref` | [CodeownersRef](#codeownersref) | The branch their CODEOWNERS file is read at, as the source's `codeowners_ref` writes it; the source's where absent (ADR-0073). |
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
| `sources.<name>.sign_in` | table | How an operator signs in at the forge to apply a re-run, by its OAuth device flow; herald reads their permission on the repository with it right before each call (ADR-0068). |
| `sources.<name>.sign_in.client_id` | string | The client id of the GitHub App, OAuth application or GitLab application with the device flow turned on; never that of the App a `write` names. A private GitHub repository needs a GitHub App's client id: its user token holds that App's read permissions alone. A GitHub App must be installed on the repository's account and hold no permission but read, its account permissions among them. |
| `sources.<name>.sign_in.least` | [Permission](#permission) | The least the operator must be allowed on the repository: "write" (GitHub's write role, GitLab's Developer role, or above), "maintain" or "admin"; "write" where absent. |
| `sources.<name>.sign_in.scopes` | array of string | The scopes asked for, each one that only reads: on GitHub `read:user` or `read:org`, none where absent; on GitLab `read_api`, `read_user`, `openid`, `profile` or `email`, `["read_api"]` where absent. The operator's token only reads who they are and what they may do on the repository; a scope that writes does not load. |
| `sources.<name>.sign_in.web` | string | The web root the device flow is asked at; where absent, the API's: `https://github.com` for `https://api.github.com`, a GHE.com tenant's `https://acme.ghe.com` for `https://api.acme.ghe.com`, and a GitHub Enterprise Server's or a self-managed GitLab's host for an API ending in `/api/v3` or `/api/v4`. |
| `sources.<name>.signals` | array of [Signal](#signal) | What the source announces of each repository: its published Releases, its version tags, the versions of its packages, or several; `["release"]` where absent. A tag and a Release of it share one identity, so each route announces it once. |
| `sources.<name>.subgroups` | boolean | Whether a GitLab group's members include the projects of its subgroups; true where absent. |
| `sources.<name>.tables` | array of string | The arrays of the member file that list repositories, such as `["repo", "ungoverned"]`; the file's own array where absent, which only JSON can hold. |
| `sources.<name>.tag_window` | [Duration](#duration) | How long a tag with no Release waits for one, counted from when herald first sees the tag; "1h" where absent, and at most "7d". |
| `sources.<name>.teams` | [TeamsFrom](#teamsfrom) | Where the teams a view's load counts come from: the config's `[teams.<name>]` alone ("config"), or also the forge's own teams of the organization or group owning the source's repositories ("forge"), a team the config names keeping the config's members. Each round a view showing its load reads the source, it reads at most 100 teams, each with at most 500 members, by one request a page; the members' logins are held for the round alone, never shown or kept, and a forge team's name and figures reach only a route that says `private = true` and `herald queue` with `--private` or `[cli] private`. GitHub's teams are each `org/team`, needing Members: read as a GitHub App or fine-grained token, or `read:org` as a classic token, on GitHub Enterprise Server and GHE.com too; GitLab's are the group and each group under it, by full path, needing `read_api`, self-managed included; Forgejo's and Gitea's are each `org/team`, needing `read:organization`. A forge that refuses folds the load as "config" does, and `herald doctor --online` names the permission; "config" where absent (ADR-0077). |
| `sources.<name>.template` | boolean | Whether template repositories are taken. |
| `sources.<name>.token` | table | A token that reaches a private repository or raises the API's rate limit, such as the run's `GITHUB_TOKEN`. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.topics` | array of string | Topics, any one of which a member holds; any where absent. |
| `sources.<name>.transport` | [Transport](#transport) | How an owner's members are polled; "auto" where absent. GitHub offers auto, graphql, rest and events; GitLab auto, graphql and rest; Forgejo auto, rest and feed. |
| `sources.<name>.update` | [Update](#update) | How a merge queue brings a pull request's branch up to date with its base through the forge: "merge" merges the base in, keeping the author's commits and signatures; "rebase" rebases the branch. GitLab offers a rebase alone; "rebase" there and "merge" elsewhere where absent (ADR-0087). |
| `sources.<name>.visibility` | array of [RepositoryVisibility](#repositoryvisibility) | The visibilities taken; only public repositories where absent. A repository whose visibility the forge does not name is private. A route that takes a private or internal member is refused unless it is `private` (`[doctor] private_to_public`). |
| `sources.<name>.webhook` | table | The secret the forge signs its webhook deliveries to `herald server` with: GitHub's and Forgejo's webhook secret, or GitLab's signing token (ADR-0007). An owner source takes its repositories' and its organization's deliveries, and announces one only where the owner's last membership record holds the repository when its work runs. |
| `sources.<name>.webhook.secret` | string | The secret's name. |
| `sources.<name>.write` | table | The credential an operator's `herald rerun --apply` re-runs a planned job with, apart from `token` and `app`: never read by a poll, a report or the server (ADR-0068). A stolen write credential re-runs without herald, so give it nothing but re-running. |
| `sources.<name>.write.app` | string | The GitHub App the writes are made as, by the name of its `[github_apps.<name>]` table, in place of `token`: an App other than the one the source reads as, installed with the permissions its writes need alone. |
| `sources.<name>.write.installation` | integer | The id of the App's installation tokens are minted for; looked up on the owner, or on the repository, where absent. |
| `sources.<name>.write.token` | table | A token that may make the writes, other than the one the source reads with. |
| `sources.<name>.write.token.secret` | string | The secret's name. |

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
| `sources.<name>.areas` | [Zones](#zones) | The areas of its repositories, each a name and the path globs, as `include` writes them, any of which a changed path falls in it by, such as `{ web = ["web/**", "packages/ui/**"] }`: at most `most_areas` areas and 1000 globs, each name and glob at most 128 and 256 characters, a path over 1024 falling in none. A view reading the source reads the paths each pull request's head changes, as one predicting conflicts does, and each row names its areas; none where absent (ADR-0071). Or "codeowners": each owner the repository's CODEOWNERS file names for a changed path, a team, a group, a role or a login, never an email, and each GitLab section, is an area (ADR-0073). |
| `sources.<name>.backfill` | integer | How many of the newest releases, tags and package versions the first poll announces; none where absent (ADR-0003). For an owner, it applies to the members the first listing finds; a member found later announces only what it publishes after it was found. |
| `sources.<name>.codeowners` | array of string | Where a repository's CODEOWNERS file is looked for, in order, the first found read: at most 8 paths in the repository; where absent, the forge's own places in its own order: GitHub `.github/`, the root, `docs/`; GitLab the root, `docs/`, `.gitlab/`; Forgejo the root, `docs/`, `.gitea/`, `.forgejo/` (ADR-0073). |
| `sources.<name>.codeowners_ref` | [CodeownersRef](#codeownersref) | The branch a repository's CODEOWNERS file is read at: "base", the branch each pull request merges into, or "default", the repository's default branch; where absent, the one the forge applies: "base" on GitHub and GitLab, "default" on Forgejo (Gitea applies "base": set it there) (ADR-0073). |
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
| `sources.<name>.milestones` | table | Which milestones are announced where the signals take "milestone": what they did, their titles, and whether their counts and a GitLab group's milestones are read; every milestone closed where absent. |
| `sources.<name>.milestones.actions` | array of [MilestoneAction](#milestoneaction) | What a milestone did that is announced: `opened`, `closed`, `reopened`, `deleted`, `overdue` (found only by a poll); `closed` where absent. |
| `sources.<name>.milestones.counts` | boolean | Whether a milestone names how many of its issues are open and closed, as its forge counts them; true where absent. On GitLab each announced milestone costs one request more. |
| `sources.<name>.milestones.group` | boolean | Whether the milestones of the GitLab group holding the project are read beside the project's own; false where absent. |
| `sources.<name>.milestones.most` | integer | The most open milestones of a repository a poll follows, in the order its forge lists them; 100 where absent, at most 1000. |
| `sources.<name>.milestones.titles` | array of string | The titles, as globs as `include` writes them, one of which a milestone announced matches; every milestone where empty. |
| `sources.<name>.most_areas` | integer | The most areas the source reads a repository's table or CODEOWNERS file naming: a table naming more is refused as the config is read, a file naming more is refused whole as it is read, and doctor says so; 200 where absent, and at most 2000, since each area is a group of a board and is matched against every changed path (ADR-0074). |
| `sources.<name>.most_codeowners` | [Size](#size) | The most of a CODEOWNERS file downloaded and read, a longer file refused unread; "128KiB" where absent, and at most "3MiB", GitHub's own limit (ADR-0073, ADR-0074). |
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
| `sources.<name>.properties` | table | Custom properties, every one of which a member holds with one of the values given, such as `{ team = "media", tier = ["core", "edge"] }`. |
| `sources.<name>.properties.<name>` | [Values](#values) | A property's values: one, or any of several. |
| `sources.<name>.pulls` | table | Which pull requests are announced where the signals take "pull-request": what they did, whether drafts are, and their labels and authors; every pull request opened, ready for review or merged where absent. |
| `sources.<name>.pulls.actions` | array of [PullAction](#pullaction) | What a pull request did that is announced: `opened`, `ready_for_review`, `reopened`, `merged`, `closed`; `["opened", "ready_for_review", "merged"]` where absent. |
| `sources.<name>.pulls.authors` | array of string | The authors, as globs, whose pull requests are announced, such as `dependabot[bot]`; every author where empty. |
| `sources.<name>.pulls.drafts` | boolean | Whether a draft's opening is announced; false where absent, so a draft is announced once it is ready for review. |
| `sources.<name>.pulls.labels` | array of string | The labels, as globs as `include` writes them, any one of which a pull request announced carries; every pull request where empty. |
| `sources.<name>.registry` | string | The host of the container registry an image is named on, as in `registry.example:5000`: where absent, ghcr.io beside GitHub's API, `containers.<host>` beside a GitHub Enterprise Server's or a GHE.com tenant's, each image's own on GitLab, and the instance's on Forgejo. |
| `sources.<name>.remembered` | integer | How many identities the cursor remembers, newest first; 500 where absent, and at least `page`. |
| `sources.<name>.repositories` | table | What the source makes of the repositories each glob names, as `include` writes them; where several name one repository, the narrowest that sets a key gives it. |
| `sources.<name>.repositories.<name>.areas` | [Zones](#zones) | Their areas, as the source's `areas` writes them, in place of the source's; the source's where absent (ADR-0071, ADR-0073). |
| `sources.<name>.repositories.<name>.codeowners` | array of string | Where their CODEOWNERS file is looked for, as the source's `codeowners` writes it; the source's where absent (ADR-0073). |
| `sources.<name>.repositories.<name>.codeowners_ref` | [CodeownersRef](#codeownersref) | The branch their CODEOWNERS file is read at, as the source's `codeowners_ref` writes it; the source's where absent (ADR-0073). |
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
| `sources.<name>.sign_in` | table | How an operator signs in at the forge to apply a re-run, by its OAuth device flow; herald reads their permission on the repository with it right before each call (ADR-0068). |
| `sources.<name>.sign_in.client_id` | string | The client id of the GitHub App, OAuth application or GitLab application with the device flow turned on; never that of the App a `write` names. A private GitHub repository needs a GitHub App's client id: its user token holds that App's read permissions alone. A GitHub App must be installed on the repository's account and hold no permission but read, its account permissions among them. |
| `sources.<name>.sign_in.least` | [Permission](#permission) | The least the operator must be allowed on the repository: "write" (GitHub's write role, GitLab's Developer role, or above), "maintain" or "admin"; "write" where absent. |
| `sources.<name>.sign_in.scopes` | array of string | The scopes asked for, each one that only reads: on GitHub `read:user` or `read:org`, none where absent; on GitLab `read_api`, `read_user`, `openid`, `profile` or `email`, `["read_api"]` where absent. The operator's token only reads who they are and what they may do on the repository; a scope that writes does not load. |
| `sources.<name>.sign_in.web` | string | The web root the device flow is asked at; where absent, the API's: `https://github.com` for `https://api.github.com`, a GHE.com tenant's `https://acme.ghe.com` for `https://api.acme.ghe.com`, and a GitHub Enterprise Server's or a self-managed GitLab's host for an API ending in `/api/v3` or `/api/v4`. |
| `sources.<name>.signals` | array of [Signal](#signal) | What the source announces of each repository: its published Releases, its version tags, the versions of its packages, or several; `["release"]` where absent. A tag and a Release of it share one identity, so each route announces it once. |
| `sources.<name>.subgroups` | boolean | Whether a GitLab group's members include the projects of its subgroups; true where absent. |
| `sources.<name>.tables` | array of string | The arrays of the member file that list repositories, such as `["repo", "ungoverned"]`; the file's own array where absent, which only JSON can hold. |
| `sources.<name>.tag_window` | [Duration](#duration) | How long a tag with no Release waits for one, counted from when herald first sees the tag; "1h" where absent, and at most "7d". |
| `sources.<name>.teams` | [TeamsFrom](#teamsfrom) | Where the teams a view's load counts come from: the config's `[teams.<name>]` alone ("config"), or also the forge's own teams of the organization or group owning the source's repositories ("forge"), a team the config names keeping the config's members. Each round a view showing its load reads the source, it reads at most 100 teams, each with at most 500 members, by one request a page; the members' logins are held for the round alone, never shown or kept, and a forge team's name and figures reach only a route that says `private = true` and `herald queue` with `--private` or `[cli] private`. GitHub's teams are each `org/team`, needing Members: read as a GitHub App or fine-grained token, or `read:org` as a classic token, on GitHub Enterprise Server and GHE.com too; GitLab's are the group and each group under it, by full path, needing `read_api`, self-managed included; Forgejo's and Gitea's are each `org/team`, needing `read:organization`. A forge that refuses folds the load as "config" does, and `herald doctor --online` names the permission; "config" where absent (ADR-0077). |
| `sources.<name>.template` | boolean | Whether template repositories are taken. |
| `sources.<name>.token` | table | A token that reaches a private repository or raises the API's rate limit, such as the run's `GITHUB_TOKEN`. |
| `sources.<name>.token.secret` | string | The secret's name. |
| `sources.<name>.topics` | array of string | Topics, any one of which a member holds; any where absent. |
| `sources.<name>.transport` | [Transport](#transport) | How an owner's members are polled; "auto" where absent. GitHub offers auto, graphql, rest and events; GitLab auto, graphql and rest; Forgejo auto, rest and feed. |
| `sources.<name>.update` | [Update](#update) | How a merge queue brings a pull request's branch up to date with its base through the forge: "merge" merges the base in, keeping the author's commits and signatures; "rebase" rebases the branch. GitLab offers a rebase alone; "rebase" there and "merge" elsewhere where absent (ADR-0087). |
| `sources.<name>.visibility` | array of [RepositoryVisibility](#repositoryvisibility) | The visibilities taken; only public repositories where absent. A repository whose visibility the forge does not name is private. A route that takes a private or internal member is refused unless it is `private` (`[doctor] private_to_public`). |
| `sources.<name>.webhook` | table | The secret the forge signs its webhook deliveries to `herald server` with: GitHub's and Forgejo's webhook secret, or GitLab's signing token (ADR-0007). An owner source takes its repositories' and its organization's deliveries, and announces one only where the owner's last membership record holds the repository when its work runs. |
| `sources.<name>.webhook.secret` | string | The secret's name. |
| `sources.<name>.write` | table | The credential an operator's `herald rerun --apply` re-runs a planned job with, apart from `token` and `app`: never read by a poll, a report or the server (ADR-0068). A stolen write credential re-runs without herald, so give it nothing but re-running. |
| `sources.<name>.write.app` | string | The GitHub App the writes are made as, by the name of its `[github_apps.<name>]` table, in place of `token`: an App other than the one the source reads as, installed with the permissions its writes need alone. |
| `sources.<name>.write.installation` | integer | The id of the App's installation tokens are minted for; looked up on the owner, or on the repository, where absent. |
| `sources.<name>.write.token` | table | A token that may make the writes, other than the one the source reads with. |
| `sources.<name>.write.token.secret` | string | The secret's name. |

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
| `sources.<name>.peers` | table | What each peer of `[peers]` may do with the source, by the peer's name, such as `{ upstream = { actions = ["push"] } }`; the peer's own `grants` must name the same. None where absent. |
| `sources.<name>.peers.<name>.actions` | array of [Action](#action) | What the peer may do, such as `["push"]`. |
| `sources.<name>.peers.<name>.namespaces` | array of string | The identities the peer may give as they are, each the start of identities up to a '/', such as "github.com/123456"; every other is taken as herald/<peer>/<identity>. None where absent. |
| `sources.<name>.peers.<name>.until` | string | When the grant ends, in RFC 3339 such as "2027-01-01T00:00:00Z" or as a TOML datetime with its offset; never where absent. |
| `sources.<name>.stale_after` | [Duration](#duration) | How long after a static log was written it is still taken: "3d" where absent, at most "30d". An older one is refused, as its host may be withholding newer pages; give more than the publisher's `refresh`. Only with `static = true`. |
| `sources.<name>.static` | boolean | Whether the feed is read from a static signed log, as a static host such as GitHub Pages serves it, rather than from a herald server: each page checked against the digest the log's signed discovery document lists. false where absent (ADR-0060). |
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
| `store.keep_decisions` | [Keep](#keep) | How long what a model answered a task is kept, and reused while the pull request it decided of is unchanged; "30d" where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keep_pulls` | [Keep](#keep) | How long a pull request kept mode keeps is kept after it closed; "30d" where absent (ADR-0061). |
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
| `store.keep_decisions` | [Keep](#keep) | How long what a model answered a task is kept, and reused while the pull request it decided of is unchanged; "30d" where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keep_pulls` | [Keep](#keep) | How long a pull request kept mode keeps is kept after it closed; "30d" where absent (ADR-0061). |
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
| `store.keep_decisions` | [Keep](#keep) | How long what a model answered a task is kept, and reused while the pull request it decided of is unchanged; "30d" where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keep_pulls` | [Keep](#keep) | How long a pull request kept mode keeps is kept after it closed; "30d" where absent (ADR-0061). |
| `store.key` | table | The secret holding a key to seal every record with, such as `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt (ADR-0005); records are not sealed where absent. |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |

### `store` of kind `durable-object`

The storage of herald's Cloudflare Durable Object, through the Worker's binding.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.binding` | string | The Worker's binding to herald's Durable Object namespace, as `wrangler.toml` names it; `HERALD` where absent. |
| `store.keep_decisions` | [Keep](#keep) | How long what a model answered a task is kept, and reused while the pull request it decided of is unchanged; "30d" where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keep_pulls` | [Keep](#keep) | How long a pull request kept mode keeps is kept after it closed; "30d" where absent (ADR-0061). |
| `store.key` | table | The secret holding a key to seal every record with, such as `HERALD_STORE_KEY`, so a record herald did not seal reads as corrupt (ADR-0005); records are not sealed where absent. |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.object` | string | The name of the object that keeps the records; `herald` where absent. |

### `store` of kind `deno-kv`

Deno KV, on Deno Deploy or wherever a database is opened.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.keep_decisions` | [Keep](#keep) | How long what a model answered a task is kept, and reused while the pull request it decided of is unchanged; "30d" where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keep_pulls` | [Keep](#keep) | How long a pull request kept mode keeps is kept after it closed; "30d" where absent (ADR-0061). |
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
| `store.keep_decisions` | [Keep](#keep) | How long what a model answered a task is kept, and reused while the pull request it decided of is unchanged; "30d" where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keep_pulls` | [Keep](#keep) | How long a pull request kept mode keeps is kept after it closed; "30d" where absent (ADR-0061). |
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
| `store.keep_decisions` | [Keep](#keep) | How long what a model answered a task is kept, and reused while the pull request it decided of is unchanged; "30d" where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keep_pulls` | [Keep](#keep) | How long a pull request kept mode keeps is kept after it closed; "30d" where absent (ADR-0061). |
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
| `store.keep_decisions` | [Keep](#keep) | How long what a model answered a task is kept, and reused while the pull request it decided of is unchanged; "30d" where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keep_pulls` | [Keep](#keep) | How long a pull request kept mode keeps is kept after it closed; "30d" where absent (ADR-0061). |
| `store.key` | table | The secret holding the key every record is sealed with, such as `HERALD_STORE_KEY`: whoever can write the repository could otherwise forge records herald trusts (ADR-0005). |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.repository` | string | The repository, as `owner/name`; the run's own where absent. |
| `store.token` | table | The secret holding a token that may push to the branch; `GITHUB_TOKEN` where absent. |
| `store.token.secret` | string | The secret's name. |

### `store` of kind `forgejo-branch`

One file per record on a `herald-state` branch of a Forgejo repository.

| Setting | Type | What it sets |
|---------|------|--------------|
| `store.api` | string | The API's base URL, such as a Forgejo's `https://forge.example/api/v1`. Where absent, it is the run's own, as Forgejo Actions sets `GITHUB_API_URL`, when the store names no repository, and Codeberg's when it names one. |
| `store.attempts` | [Attempts](#attempts) | How many times a write that lost a race to an unrelated write is tried. |
| `store.branch` | string | The branch the records are kept on; `herald-state` where absent. herald's first write makes it in an empty repository; in one that holds other branches it is made once, with no history, such as with `git switch --orphan herald-state`. |
| `store.keep_decisions` | [Keep](#keep) | How long what a model answered a task is kept, and reused while the pull request it decided of is unchanged; "30d" where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keep_pulls` | [Keep](#keep) | How long a pull request kept mode keeps is kept after it closed; "30d" where absent (ADR-0061). |
| `store.key` | table | The secret holding the key every record is sealed with, such as `HERALD_STORE_KEY`: whoever can write the repository could otherwise forge records herald trusts (ADR-0005). |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.repository` | string | The repository, as `owner/name`; the run's own where absent. |
| `store.token` | table | The secret holding a token that may push to the branch; `GITHUB_TOKEN`, as Forgejo Actions sets it, where absent. |
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
| `store.keep_decisions` | [Keep](#keep) | How long what a model answered a task is kept, and reused while the pull request it decided of is unchanged; "30d" where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keep_pulls` | [Keep](#keep) | How long a pull request kept mode keeps is kept after it closed; "30d" where absent (ADR-0061). |
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
| `store.keep_decisions` | [Keep](#keep) | How long what a model answered a task is kept, and reused while the pull request it decided of is unchanged; "30d" where absent. |
| `store.keep_deliveries` | [Duration](#duration) | How long a webhook delivery's id is kept after it was admitted, so a replay of it is refused (ADR-0013); "7d" where absent, and at least "3d", longer than any sender goes on retrying a delivery. |
| `store.keep_ledger` | [Keep](#keep) | How long a ledger entry, and where each of its deliveries to a follower's inbox stands, is kept after a run last claimed it; "90d" where absent. |
| `store.keep_notes` | [Keep](#keep) | How long a Note herald's ActivityPub actors published is kept, and served by its id; "forever" where absent. |
| `store.keep_pulls` | [Keep](#keep) | How long a pull request kept mode keeps is kept after it closed; "30d" where absent (ADR-0061). |
| `store.key` | table | The secret holding the key every record is sealed with, such as `HERALD_STORE_KEY`: whoever can write the repository could otherwise forge records herald trusts (ADR-0005). |
| `store.key.secret` | string | The secret's name. |
| `store.kind` | string |  |
| `store.lock_lease` | [LockLease](#locklease) | How long the store's lock holds before it is taken as abandoned. |
| `store.lock_patience` | [LockPatience](#lockpatience) | How many times a lock with no time to judge its age by is found held, unchanged, before it is taken as abandoned. |
| `store.repository` | string | The repository, as `owner/name`; the run's own where absent. |
| `store.token` | table | The secret holding a token with the repository's Variables permission; the job's own token cannot write variables. |
| `store.token.secret` | string | The secret's name. |

## `tasks`

The closed-set questions views ask of models where their rules cannot tell, by name.

| Setting | Type | What it sets |
|---------|------|--------------|
| `tasks.<name>.instructions` | string | What to decide, in a sentence. |
| `tasks.<name>.least` | number | The least probability an answer is taken at, from 0 to 1, where a model gives one; where a model gives none, no answer is taken. |
| `tasks.<name>.models` | array of string | The models asked, in turn, until one answers, such as `["jev", "local"]`. |
| `tasks.<name>.most` | integer | The most pull requests it is asked of a round: 50 where absent; the rest read as unknown. |
| `tasks.<name>.options` | table | The options it may be answered with, each a key and what it means. |
| `tasks.<name>.options.<name>` | string |  |

## `teams`

The teams a view's load counts, by name: each named as a review request and a CODEOWNERS file name it, such as `org/team`, at most 200.

| Setting | Type | What it sets |
|---------|------|--------------|
| `teams.<name>.members` | array of string | Its members, each a login, or `login@host` for one forge's login alone, at most 500. |

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
| `views.<name>.advisories_from` | [AdvisoriesFromFile](#advisoriesfromfile) | Which repositories its advisories overview reads: "owner", where absent, its organization's or group's own listings, and a user's members one by one, the newest pushed first; "shown", those it shows a pull request of; or "members", its owner's newest pushed. A view that filters its repositories by name reads those it holds one by one, the first by name. A one-repository source reads its repository. |
| `views.<name>.advisory_repositories` | integer | The most repositories its advisories overview reads one by one: 20 where absent, from 1 to 100. Each costs three requests on GitHub and one on GitLab, and their order one more. |
| `views.<name>.areas` | array of string | The areas, any one of which a pull request it shows falls in, by name, each a glob ignoring case that matches an area one of its sources names; every pull request where empty (ADR-0071). |
| `views.<name>.authors` | array of string | The authors it shows, by login, each a glob ignoring case; every one where empty. |
| `views.<name>.bots` | [Bots](#bots) | What it does with a bot's pull requests: "batch" where absent. |
| `views.<name>.broken_at` | integer | How many open pull requests one check fails on before the base they merge into reads as broken: 3 where absent (ADR-0065). |
| `views.<name>.bumps` | array of string | Its own bump patterns, read before Dependabot's and Renovate's, such as "chore: bump {package} to {to}". |
| `views.<name>.capacity_from` | [CapacityFrom](#capacityfrom) | Which repositories its capacity overview reads: "shown", those it shows a pull request of, where absent; or "members", its owner's newest pushed, at most `capacity_repositories`, or where it filters its repositories by name the first by name of those it holds. |
| `views.<name>.capacity_repositories` | integer | The most members "members" reads, the newest pushed first, or the first by name where the view filters its repositories by name: 20 where absent, from 1 to 100. Each costs two requests on GitHub and GitLab and one on Forgejo, and the push order one more. |
| `views.<name>.conflicts` | boolean | Whether it predicts conflicts: it reads the paths each pull request's head changes, at most 100 each, and each row names the open pull requests of its repository merging into its branch that change a path in common, a renamed file's old path among them; false where absent. It costs a request a pull request read in full, but on GitHub with a token, which reads them with each pull request and costs a request for each pull request renaming a file, whose old path GraphQL does not name, at most `most_pulls` a round; a kept record keeps them until its head moves. |
| `views.<name>.decide` | table | The task that decides each slot its rules leave untold, such as `{ turn = "whose", priority = "urgency" }`; nothing is asked of a model where absent. |
| `views.<name>.decide.<name>` | string |  |

### `views.<name>.deciding`: herald's own rules, or the forge's own status

| Setting | Type | What it sets |
|---------|------|--------------|
| `views.<name>.deciding` | [DecidingName](#decidingname) | herald's own rules, or the forge's own status. |

### `views.<name>.deciding`: The labels maintainers set

| Setting | Type | What it sets |
|---------|------|--------------|
| `views.<name>.deciding` | table | The labels maintainers set. |
| `views.<name>.deciding.labels` | array of tables | Each label and its state, in the order they are tried. |
| `views.<name>.deciding.labels[].label` | string | The label, as the forge names it, ignoring case. |
| `views.<name>.deciding.labels[].state` | [Turn](#turn) | The state. |

| Setting | Type | What it sets |
|---------|------|--------------|
| `views.<name>.flaky` | table | What makes a failing check a flaky candidate: failing then passing on one head at `least` times (1 where absent) within `window` ("7d" where absent), as kept records saw it. |
| `views.<name>.flaky.least` | integer | How many times within the window make a candidate: 1 where absent. |
| `views.<name>.flaky.window` | [Duration](#duration) | How far back a check failing then passing on one head counts: "7d" where absent. |
| `views.<name>.group_by` | [GroupBy](#groupby) | What it groups its rows by: "turn" where absent. |
| `views.<name>.labels` | array of string | The labels, any one of which a pull request it shows carries, each a glob ignoring case; every pull request where empty. |
| `views.<name>.listed_visibility` | boolean | Whether a live read through a REST listing without a token, as of GitHub, GitHub Enterprise Server, GHE.com or Forgejo, takes who may see a repository from what the listing states of it, rather than taking each of its rows as private: false where absent. It never widens a kept record, a read with a token, a GitLab listing, or an owner's rows, which its membership record names. |
| `views.<name>.load` | table | Its load overview, per team and per area and never per person: the review requests open now on the rows it shows, and the reviews given on them over `window` where every source is read kept. Off where absent; `load = {}` shows it as shipped. |
| `views.<name>.load.least_team` | integer | The fewest people a team or an area is shown by name with: 3 where absent, from 3 to 50. A team of fewer members, a person's area, a GitLab name no lookup told apart, and a team, group or role area of fewer known members fold into "other", which shows only where the people folded into it are as many. |
| `views.<name>.load.unknown_teams` | boolean | Whether a team, group or role area that no `[teams.<name>]` of its name gives members to is shown as it is: false where absent, and it folds. Such an area may be one person. A person's area always folds. |
| `views.<name>.load.window` | [Duration](#duration) | How far back reviews given count: "7d" where absent, from "1d" to "90d". |
| `views.<name>.mode` | [ViewMode](#viewmode) | Where it reads its pull requests: "auto" where absent, kept of each source that takes webhooks where the store keeps pull requests and live otherwise; "live" from the forge each time; or "kept" from the records the store keeps, folded from webhooks and reconciled with a listing. |
| `views.<name>.most_advisories` | integer | The most open advisories its overview reads, a page of 100 a request: 1000 where absent, at most 10000. Past it the section says more were open. |
| `views.<name>.most_open` | integer | The most open pull requests it lists of each repository: 500 where absent. |
| `views.<name>.most_pulls` | integer | The most it reads in full where each takes requests of its own, as on Forgejo and on GitHub without a token, those past it read as unknown; and on GitHub with a token, predicting conflicts, the most whose renamed files' old paths it reads, those past it read with their paths not whole: 100 where absent. |
| `views.<name>.overviews` | array of [OverviewName](#overviewname) | The overviews it shows beside its rows, such as `["capacity"]`: none where absent (ADR-0066). |
| `views.<name>.queue` | boolean | Whether it reads each pull request's entry in its forge's merge queue: its position, state, when it entered and the forge's estimate, on GitHub with a token in the query that reads it, at no request of its own; false where absent. Without it a row still says whether a pull request is in the queue where its forge does. A row shows the entry, and why it last left where a webhook said, as it shows the rest of the pull request (ADR-0078). |
| `views.<name>.reconcile` | [Duration](#duration) | How long kept mode waits at least before listing a source's open pull requests again, each page conditional on its last answer; every round where absent. |
| `views.<name>.release` | table | Its release: it shows only the open pull requests its selectors pick, and says whether the release is ready or what holds it back, such as `release = { milestone = "v2.0" }` (ADR-0081). None where absent. |
| `views.<name>.release.blocked_by` | array of [Blocker](#blocker) | What holds it back: every one where absent. "label", a pull request carrying a blocker label; "failed", one whose check failed; "draft", a draft; "changes", one a reviewer asked to change; "base", a base branch its pull requests merge into whose head has a failed check, each read once a round by one request (two on GitHub, one more on GitLab where its pipeline failed), asked on the validators kept where the store keeps pull requests, at most 20 branches; one that could not be read holds it back too (ADR-0083). A pull request not read in full holds it back whatever this says. |
| `views.<name>.release.blocker_labels` | array of string | The labels that block it, each a glob ignoring case: `["blocker", "release-blocker"]` where absent, at most 50. |
| `views.<name>.release.branch` | string | The branch its pull requests merge into, a glob ignoring case, such as "release/*". |
| `views.<name>.release.combine` | [Combine](#combine) | How its selectors combine: "all" where absent, each picking a pull request, or "any". |
| `views.<name>.release.header` | boolean | Whether the board heads it with each open milestone its milestone selector matches: its title, due day, whether it is overdue and its open and closed issues, each repository it shows a pull request of read by one more request a round (ADR-0082). True where absent; it needs a milestone selector. |
| `views.<name>.release.label` | string | A label its pull requests carry, a glob ignoring case. |
| `views.<name>.release.milestone` | string | The milestone its pull requests carry, by title, a glob ignoring case, such as "v2.0". |

### `views.<name>.release.pull`: The tool that opens it

| Setting | Type | What it sets |
|---------|------|--------------|
| `views.<name>.release.pull` | [ReleaseTool](#releasetool) | The tool that opens it. |

### `views.<name>.release.pull`: Its head branch

| Setting | Type | What it sets |
|---------|------|--------------|
| `views.<name>.release.pull` | table | Its head branch. |
| `views.<name>.release.pull.head` | string | The glob. |

### `views.<name>.release.pull`: Its title

| Setting | Type | What it sets |
|---------|------|--------------|
| `views.<name>.release.pull` | table | Its title. |
| `views.<name>.release.pull.title` | string | The glob. |

| Setting | Type | What it sets |
|---------|------|--------------|
| `views.<name>.release.repository` | string | The one repository the release is held to, by full path such as "acme/api", ignoring case: its pull requests, and the milestones and issues its header and rows read, that repository's alone. Absent, the release spans every repository the view holds: on an owner source each member, and with `milestones.group` on a GitLab owner the own milestones of each group at or under it, seen as private, at most 20 read a round with public ones first, each repository seen as its own (ADR-0086). |
| `views.<name>.release.rows` | [ReleaseRows](#releaserows) | What the board lists as its rows: "pulls" where absent, the open pull requests it picks; or "issues", the open issues carrying each open milestone its milestone selector matches, by one more request a round for each such milestone, at most 50 a repository, each seen as its repository or as private where it is confidential. Its pull requests still judge whether it is ready. "issues" needs a milestone selector (ADR-0084). |
| `views.<name>.release.withheld` | [ReleaseWithheld](#releasewithheld) | How a surface some of its pull requests are withheld from, a private or internal repository's, is told the verdict: "unknown" where absent, not known because some pull requests are private; "shown", judged by the pull requests it shows alone; "masked", the whole verdict, each withheld one holding it back said only to be private. |
| `views.<name>.repositories` | array of string | The repositories it shows, by full path, each a glob ignoring case; every one where empty. |
| `views.<name>.rules` | table | herald's own rules, each of which it may switch off. |
| `views.<name>.rules.approval` | boolean | One waits on review until it is approved. |
| `views.<name>.rules.changes` | boolean | One a reviewer asked to change waits on its author. |
| `views.<name>.rules.conflict` | boolean | One that conflicts waits on its author. |
| `views.<name>.rules.draft` | boolean | A draft waits on its author's readiness. |
| `views.<name>.rules.failed` | boolean | One whose check failed waits on its author. |
| `views.<name>.rules.pending` | boolean | One whose checks are running waits on them. |
| `views.<name>.rules.queued` | boolean | One in the merge queue waits on the queue. |
| `views.<name>.sources` | array of string | The sources it reads, each naming one repository or an owner on a forge; every such source where absent. |
| `views.<name>.stale` | [Duration](#duration) | How long one waits with nobody acting before it is stale: "14d" where absent. |
| `views.<name>.stale_only` | boolean | Whether it shows only the stale. |
| `views.<name>.states` | array of [Turn](#turn) | The states it shows, every one where empty. |

## Types

### Action

What a peer may be granted to do with a resource of this herald.

### AdvisoriesFromFile

Which repositories an advisories overview reads.

### AlertAction

What an alert did that is announced.

### AppliedBy

Where a queue's steps are taken.

### Approve

Who decides whether a follow of herald's actors is taken.

### ArchiveAfter

How long a quiet thread stays open before Discord archives it: one of the four lengths Discord takes.

### AreaPaths

Where a route narrowing by areas reads the paths a pull request's head changes, which its event's areas are those of (ADR-0072).

### Attempts

How many times the store tries a write that lost a race to an unrelated write, 3 by default.

### AttributeType

What type an attribute holds.

### Audience

Who sees an actor's Notes on the timelines. Followers see every Note.

### Blocker

What holds a release back.

### Bots

What a view does with a bot's pull requests.

### CapacityFrom

Which repositories a capacity overview reads.

### Cloud

The Microsoft cloud a Teams bot is registered in: each names the hosts its bot signs in at and posts through, as Microsoft's documentation gives them (ADR-0093).

### CodeownersRef

The branch a repository's CODEOWNERS file is read at.

### Colour

A colour, written "#rrggbb".

### Combine

How a release's selectors combine.

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

### IdTokenSource

Where a run's ID tokens come from.

### IssueAction

What an issue did that is announced.

### Keep

A length of time, or "forever".

### KeyCache

Where `herald server` keeps the keys inbound activities are checked with.

### Kind

What kind of thing an event announces: release, package-version, feed-entry, message, run, alert, or a kind the config declares.

### Lasting

A length of time, none at all among them: a number and ms, s, m, h, d or w, as "10s" or "0s".

### LateRelease

What a route does with a Release whose tag it already announced.

### Level

How grave a line of herald's log is, the least grave first.

### LockLease

How long the store's lock holds before it is taken as abandoned, "10m" by default and at most "40m".

### LockPatience

How many times a lock with no time to judge its age by is found held, unchanged, before it is taken as abandoned, 10 by default.

### LogFormat

How herald's log writes each line.

### MatrixVia

How the homeserver carries commands to herald.

### MemberHooks

Who registers a project webhook on each member of a GitLab owner.

### MergeRuns

Who runs a branch's queue.

### MergeUpdated

Whether a gesture vouches for the head herald's own update of the branch made from the head it vouched for.

### Message

The commit message a merge is made with.

### Method

How a queue merges a pull request.

### MilestoneAction

What a milestone did that is announced.

### Mode

Whom herald's actors federate with.

### OneCharacter

One character, as a config writes it.

### OrgShared

How a private channel shared across an Enterprise Grid organization counts before an advisories section is sent there (ADR-0067).

### OverviewName

An overview a view shows beside its rows.

### PackageVisibility

Who may see a package's version where its own visibility and its repository's differ.

### Permission

The least a signed-in operator must be allowed to do on a repository for herald to re-run a job of it on their word.

### Pin

Whether a route pins, and how it finds its earlier pin.

### PinLookups

How many pins the destination looks up, one request each, as it finds the route's earlier pins, 25 by default.

### Prefer

Which of a tag and its Release a source announces where it announces both.

### PrereleaseRule

How a source tells a pre-release.

### Prereleases

Which pre-releases a route takes.

### PullAction

What a pull request did that is announced.

### PushCredential

What proves who pushes to a peer.

### Rate

How many in a length of time: a number, a slash and s, m, h or d, as "120/m".

### ReadBack

How many pages of its own posts the destination reads back, 5 by default.

### RecordKind

A kind of record a store may keep beside its ledger, cursors, schedules and deliveries, which every store that keeps records keeps.

### ReleaseRows

What a release's board lists as its rows.

### ReleaseTool

A release pull request by the tool that opens it.

### ReleaseWithheld

How a release is told where some of its pull requests are withheld.

### Replay

How often a token an app takes may be presented.

### Replies

Who sees herald's answers to `latest` and `changelog`; the answers to `subscribe` and `unsubscribe` are always the asker's alone.

### RepositoryVisibility

Who may see a repository.

### RerunMode

How a rule acts on a flaky check.

### RoleLost

What becomes of a pull request herald handed to the forge's queue once the person whose gesture it is no longer holds the queue's least role, with no cancel herald carries out standing.

### Rotation

Whether a peer's own signed statement may replace the keys pinned for it.

### RuleDequeued

What a rule alerts on of the pull requests that left the merge queue.

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

### Taking

What a token an entry takes may do.

### TeamsFrom

Where the teams a view's load counts come from.

### TeamsVia

How Teams carries what it posts to a bot.

### TokenAlgorithm

The algorithm an ID token is signed with, of those herald checks.

### TokenHash

### Tool

What raised an alert, in words every forge's alerts are read as.

### Transport

How an owner's members are polled.

### Turn

Whose turn an open pull request is.

### Update

How a pull request's branch is brought up to date with its base.

### Values

A property's values: one, or any of several.

### VendorListing

How a Packagist vendor's packages are listed.

### Verify

Which envelopes a pull takes.

### Version

### ViewMode

Where a view reads its open pull requests (ADR-0061).

### Visibility

Who sees a status.

### Vouches

Which head a gesture vouches for.

### Withdraw

What a route does with an announcement whose release the forge deleted (ADR-0048).

### Zones

A source's `areas` as the schema shows it.

## Shipped presets

Each preset herald ships, the parameters a `[presets.<name>]` table gives it, and the tables it expands to, each named `<name>-` and then as written here.

### `herald:announce-releases`

Each release of its sources, announced once to its destination with its notes.

| Parameter | Given | What it sets |
|---|---|---|
| `destination` | always | The destination its routes announce to, as `[destinations]` names it. |
| `sources` | always | The sources whose events its routes take, as `[sources]` names them. |
| `prereleases` | optional | Which pre-releases it announces: `include`, `exclude` or `only`. |
| `title` | optional | The template of each announcement's title. |
| `body` | optional | The template of each announcement's body. |

```toml
[[routes]]
id = "releases"
kinds = ["release"]
prereleases = "include"
template = "release"

[templates.release]
title = "{{ event.project }} {{ event.attributes.version }}{% if event.attributes.prerelease %} · pre-release{% endif %}"
body = "{{ event.body }}"
```

### `herald:review-reminders`

Each open pull request waiting on review past each of its stages, nudged at 09:00 on weekdays to its destination.

| Parameter | Given | What it sets |
|---|---|---|
| `destination` | always | The destination its routes announce to, as `[destinations]` names it. |
| `sources` | optional | The sources whose open pull requests it watches, as `[sources]` names them; every forge source where not given. |
| `stages` | optional | How long a pull request waits on review, since anyone last acted on it, before each nudge. |
| `at` | optional | The wall-clock time, as "HH:MM", it nudges at. |
| `days` | optional | The days it nudges on. |
| `zone` | optional | The IANA time zone `at` and `days` are read in, such as "Europe/Amsterdam". |

```toml
[[routes]]
id = "reminders"
sources = ["reviews"]
template = "herald:nudge"

[views.reviews]
states = ["waiting-on-review"]

[rules.reviews]
view = "reviews"
stages = ["2d", "7d"]
at = "09:00"
days = ["mon", "tue", "wed", "thu", "fri"]
zone = "UTC"
```

### `herald:merge-queue-small-team`

A merge queue on one base branch, which a writer enters a pull request into by a label or a comment.

| Parameter | Given | What it sets |
|---|---|---|
| `source` | always | The source whose repository it merges in, of kind `github`, `gitlab` or `forgejo`, as `[sources]` names it. |
| `branch` | always | The base branch it merges into, such as `main`. |
| `method` | always | How it merges: `merge`, `squash`, `rebase`, `fast-forward`, or `repository`, the repository's one allowed method. |
| `write` | always | The credential its steps are made with, held apart from every one herald reads with: `{ token = { secret = "NAME" } }` or `{ app = "<github_apps name>" }`. |
| `runs` | optional | Who runs it: `forge`, the forge's own merge queue or merge train, where not given; or `herald`, herald's own serial queue. |
| `enter` | optional | The gesture that enters a pull request: `{ label = "herald:merge" }` where not given, or another label or a comment's command. |

```toml
[merge_queues.queue]
```

### `herald:security-alerts-private`

Each security alert of its sources, announced to its destination, which its route states is private.

| Parameter | Given | What it sets |
|---|---|---|
| `destination` | always | The destination its routes announce to, as `[destinations]` names it. |
| `sources` | always | The sources whose events its routes take, as `[sources]` names them. |
| `title` | optional | The template of each announcement's title. |
| `body` | optional | The template of each announcement's body. |

```toml
[[routes]]
id = "alerts"
kinds = ["alert"]
private = true
template = "alert"

[templates.alert]
title = "{{ event.project }}: {{ event.title }}"
body = "{% if event.attributes.severity %}Severity: {{ event.attributes.severity }}.{% endif %}"
```
