# ADR-0004: Each destination gets its own dialect, limits and features, described as data

**Status:** Accepted; decision 8 superseded in part by [ADR-0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md)
**Date:** 2026-10-31

## Context

Discord, Matrix, Slack and the Fediverse all take a message, and differ in
nearly everything else:

- **Markup.** Discord renders its own Markdown, Slack renders `mrkdwn`, Matrix
  renders HTML beside a plain body, and Mastodon renders plain text.
- **Threads.** Discord opens a thread from a message or as a forum post. Slack
  and Matrix reply into a thread. The Fediverse threads by replying.
- **Crossposting.** Only Discord's announcement channels crosspost to
  following servers.
- **Mentions.** Discord pings roles. Slack and Matrix mention people. The
  Fediverse has followers, not mentions.
- **Limits.** The length of a title, a body, a block and a post differs on
  each, and so does the rate at which each answers `429`.

herald's first user wants every Discord feature at once: a crosspost, a
thread per release, a package-first title, a role ping per package, and
pre-releases marked. Other adopters want the zero-setup path: a webhook URL
and nothing else.

## Decision

1. **A destination is described to the core as a `Profile`** (ADR-0001). The
   `Profile` holds the destination's dialect, its limits, and the features it
   offers: threads, crosspost, edits, pins, reading back its own posts, how it
   mentions, and its interactions. The core renders, chunks and plans against
   the `Profile`. Each destination crate supplies its own, and a doctor check
   reads the same `Profile` (ADR-0002).

2. **One template, rendered once, converted per dialect.** A template
   renders Markdown (ADR-0006). The render step converts it to the
   destination's dialect:
   - Discord's Markdown;
   - Slack's `mrkdwn`, with `&`, `<` and `>` escaped;
   - Matrix's sanitised HTML, beside a plain body;
   - plain text for Mastodon, with links kept whole.

   A route may give a dialect its own template where the conversion is not
   what it wants.

3. **Chunking splits where a reader would.** Text over a limit is split at a
   paragraph, then a line, then a word, and never inside a grapheme. A code
   fence split in two is closed before the split and reopened after it. The
   first part is the announcement. The rest go where the destination keeps a
   conversation: into the announcement's thread, or as follow-up messages
   where there is none. Property tests hold this (ADR-0008).

4. **Discord**, by bot token or by webhook URL.
   - **Channels.** An announcement channel and a text channel take a message
     with an embed: the title (the package-first title of the first user's
     routes), its link, the body, a colour, and the time. A forum channel
     takes a forum post, whose title becomes the post's name.
   - **Crosspost.** In an announcement channel, `crosspost = true` publishes
     the message to every server that follows the channel.
   - **Threads.** `thread = { name = "…" }` opens a thread from the
     announcement, named from its own template and cut to Discord's 100
     characters. The overflow chunks are posted into the thread.
   - **Mentions.** A route names the roles it pings. The message's
     `allowed_mentions` is pinned to those roles, with `parse` left empty, so
     no text in an event can ping `@everyone`, `@here`, a user or any other
     role.
   - **Components.** Link buttons for the release page and the changelog. At
     layer two and up, a button to subscribe to the package's role.
   - **Interactions**, at layer two and up, over HTTP. The slash commands are
     `/subscribe`, `/unsubscribe`, `/latest` and `/changelog`.
     `herald discord commands` registers them. Subscribing toggles the
     package's opt-in role on the member.
   - **Housekeeping.** It archives or locks a release thread after the route's
     `archive_after` or `lock_after`, and pins the newest release, unpinning
     the one it replaces.
   - **Bot or webhook.** A webhook URL posts with nothing to set up, into a
     text or announcement channel or as a forum post. It cannot crosspost,
     open a thread from a message, list the channel's posts for destination
     markers, or receive interactions. Doctor names each of these for a route
     that asks for one (ADR-0002).

5. **Slack**, by bot token or by incoming-webhook URL.
   - **Messages.** Messages are posted with `chat.postMessage` in Block Kit: a
     header with the title, sections in `mrkdwn`, and a plain-text fallback.
     Link unfurling is off.
   - **Threads.** The overflow chunks reply in the announcement's thread.
   - **Mentions.** Slack has no roles. Subscribers are kept in the store
     (ADR-0005), and each is mentioned as `<@U…>` on the announcement.
   - **Interactions**, at layer two and up: the same four commands, as Slack
     slash commands and Block Kit buttons, over signed HTTP requests.
   - **Housekeeping.** It pins the newest release. Slack has no thread to
     archive or lock, and doctor says so for a route that asks.
   - **Bot or webhook.** An incoming webhook returns no message timestamp, so
     it cannot thread, pin, edit, or carry a marker herald can read back.
     Doctor names each of these.

6. **Matrix**, by a bot account's access token on the configured homeserver.
   - **Messages.** An announcement is an `m.room.message` with a plain body
     and sanitised HTML. It is sent with `PUT …/send/…/{txnId}`, where the
     transaction id is derived from the never-twice key, so a retried request
     is applied once by the homeserver itself.
   - **Threads.** The overflow chunks join the announcement's thread
     (`m.thread`).
   - **Mentions.** Subscribers kept in the store are mentioned through
     `m.mentions.user_ids`.
   - **Commands**, at layer two and up, through an application service. The
     homeserver pushes room events to the endpoint, verified by the
     registration's token, and herald answers `!herald subscribe`,
     `!herald unsubscribe`, `!herald latest` and `!herald changelog`.
     `herald matrix registration` writes the registration file the
     homeserver's administrator installs.
   - **Housekeeping.** It pins the newest release through the room's pinned
     events. Matrix has no thread to archive, and doctor says so for a route
     that asks.

7. **The Fediverse**, in two ways.
   - **Through the Mastodon client API, at every layer.** herald posts as an
     account the adopter owns, on any server speaking that API: Mastodon,
     GoToSocial, Akkoma or Pleroma.
     - Each status is posted with an `Idempotency-Key` derived from the
       never-twice key.
     - The overflow is a reply chain under the announcement.
     - The visibility, language and an optional content warning are route
       options.
     - The character limit is the server's own, which `doctor --online` reads
       from the server and a route may state.
   - **As herald's own ActivityPub actor, at layer two and up.**
     - Each project announced this way is an actor on the endpoint's domain,
       found through WebFinger, so `@php-mutation-gate@herald.example` can be
       followed.
     - A follow is the opt-in. An incoming `Follow` is verified by its HTTP
       signature, stored as a subscription (ADR-0005) and answered with
       `Accept`. `Undo` removes it.
     - An announcement is a `Create` of a `Note`, delivered to each follower's
       shared inbox. Each delivery is signed with RSA-SHA256 HTTP signatures,
       as Mastodon verifies them.
     - Each delivery is a step of the announcement in the ledger. A run cut
       short by a platform's time limit leaves the rest for the next run,
       and no inbox gets the post twice.
     - The actor's private key is a secret (ADR-0007). `herald activitypub
       keygen` writes a new key to a file, for the adopter to store as one.

8. **Destination markers live in the most invisible place each platform
   keeps.** For a store that reads the destination's own posts (ADR-0005):

   | Destination | The key is carried in |
   |-------------|-----------------------|
   | Discord | the fragment of the embed title's link (`…/releases/tag/v1.2.0#herald-<key>`), seen only on hover |
   | Slack | the message's metadata, which no reader sees |
   | Matrix | a namespaced field of the event's content, `io.nightworks.herald.key` |
   | Mastodon | the fragment of the post's link |

   Reading markers back needs Discord's *Read Message History* permission,
   which doctor checks with `--online`.

9. **Rate limits and outages are answers, not failures.** A `429` or a `5xx`
   becomes a *retry after* outcome. The flow waits within its time budget, or
   leaves the step for the next run, which the ledger makes safe (ADR-0005).

10. **Each destination's outbound hosts are fixed or configured.**
    - Discord and Slack reach only their own API hosts, unless a destination
      names another base URL. The recorded-fixture server in CI does
      (ADR-0010).
    - Matrix reaches its homeserver, and Mastodon its server.
    - The ActivityPub actor reaches the inboxes of its stored followers, and
      only on public addresses (ADR-0007).

## Alternatives considered

| Option | Why it lost |
|--------|-------------|
| **A template per destination, always** | The same release would be written four times, and the four would drift. Most announcements read the same everywhere once the markup is converted, so one template is the default and a per-dialect template is the exception. |
| **Bot tokens only** | One code path, and an application to create before the first post. The webhook is the zero-setup path, and doctor names exactly what it gives up. |
| **Slack user groups for subscriptions** | They need a paid Slack plan, and herald's own list works on every plan. |
| **Only the Mastodon client API for the Fediverse** | It works at every layer but needs an account per project, and a follower cannot follow one package without following the account. The actor gives each project its own followable identity where an endpoint exists. |
| **Only the ActivityPub actor** | It needs an inbound endpoint, so an Actions-only adopter would have no Fediverse at all. |
| **A visible footer as the marker** | It works the same everywhere, and it is visible in every post on every platform. Each platform has a quieter place, and herald uses it. |
| **Discord's gateway for interactions** | The gateway needs a process that is always connected. HTTP interactions need only an endpoint that answers, and every serverless platform has one. |

## Consequences

**The first user's announcement is one route.** It crossposts, opens a
thread, uses a package-first title, pings a role, and marks a pre-release
(ADR-0010).

**A destination's quirks stay in its crate.** The core reads a `Profile`, and
a fifth destination is a fifth crate and its fixtures.

**Each platform's own idempotency is used where it exists.** Matrix
transaction ids and Mastodon idempotency keys close the window between a post
landing and the ledger recording it (ADR-0005).

## Related

- [ADR-0001](0001-a-sans-io-core-behind-ports.md): destinations reach their platform through `Http`
- [ADR-0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md): what each layer serves, and what doctor names
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): markers, subscriptions and the ledger
- [ADR-0006](0006-one-herald-toml-with-sandboxed-templates-and-a-published-schema.md): templates and route options
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): verifying interactions and signatures
- [ADR-0011](0011-markers-count-only-on-herald-s-own-posts-a-route-keeps-one-pin-and-a-lock-archives.md): markers count only on herald's own posts, a route keeps one pin, and a lock archives
