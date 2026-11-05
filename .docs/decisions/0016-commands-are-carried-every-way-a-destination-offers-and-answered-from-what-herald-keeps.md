# ADR-0016: Commands are carried every way a destination offers, and answered from what herald keeps

**Status:** Accepted
**Date:** 2026-11-05

## Context

ADR-0004 gives Discord, Slack and Matrix four commands, `subscribe`,
`unsubscribe`, `latest` and `changelog`, and a button that subscribes. It
carries Discord's over HTTP interactions alone, Slack's over signed HTTP
requests, and Matrix's through an application service. Each of those needs
a public endpoint, and the application service also needs the homeserver's
administrator.

What sets herald apart is how many ways it runs (ADR-0002). A destination's
commands should run in as many of them as the destination allows. ADR-0004
also leaves open what `latest` answers with, which project a command acts
on where a channel carries several, and who sees each answer.

## Decision

1. **Each destination takes its commands every way it offers, chosen by
   `[destinations.<name>.interactions] via`.**

   | Destination | Posted to herald's endpoint | Held by `herald server` |
   |-------------|-----------------------------|-------------------------|
   | Discord | `http`: each interaction posted to `/interactions/<destination>`, signed with the application's key | `gateway`: a gateway session as the bot, with no public endpoint |
   | Slack | `http`: each command and button press posted to `/interactions/<destination>`, signed with the signing secret | `socket`: a Socket Mode connection opened with an app-level token |
   | Matrix | `appservice`: each transaction pushed to `/matrix/<destination>/_matrix/app/v1/transactions/<id>`, with the homeserver's token | `sync`: a sync as the bot, with nothing for the administrator to install |

   Doctor names a transport the layer cannot carry: an endpoint where none is
   served, and a session anywhere but `herald server`.

2. **Every request is verified and admitted before it is read** (ADR-0007,
   ADR-0013): a Discord interaction by its Ed25519 signature, a Slack request
   by its HMAC, a Matrix transaction by the homeserver's token and admitted
   once by its id. A Slack response URL is followed only on Slack's own
   hooks.

3. **A command is answered first and worked after.** An endpoint answers a
   Discord interaction as being worked and edits that answer once it is
   worked; a session defers it itself. Slack and Matrix are answered at once
   and sent the answer once it is worked (ADR-0014). Autocompletion is
   answered at once.

4. **`latest` and `changelog` answer from a `latest` record each project
   keeps.** Each announcement that lands keeps, by compare-and-set, the
   project's newest event, its newest that is not a pre-release, and the
   routes it was announced on. An announcement that could not keep it stops,
   and the next run of the same event keeps it. `latest` answers the title
   of the route's template, linked to the release, with its day;
   `changelog` answers the template's body and a link to the changelog or
   the release. `prereleases` and `template` change both. The markers store
   keeps no such record, so a config answering either over it is refused.

5. **A command acts on the project it names, or the only one announced
   where it was given.** A Discord command completes the projects announced
   in its channel. Where several are announced and none is named, herald
   names them.

6. **Subscribing is by Discord's roles or herald's store.**
   - On Discord, a route's `subscribe = { role = "<id>" }` names the opt-in
     role a member is given and the route mentions. `herald discord roles`
     lists a server's roles and makes a mentionable one. Giving and making
     roles needs Manage Roles, and nothing else does.
   - On Slack and Matrix, a member subscribes to be mentioned by a route that
     mentions its subscribers, kept in the store (ADR-0005).
   - A button on each announcement subscribes, or unsubscribes a member
     already subscribed, where `subscribe.button` leaves it on.

7. **Answers to `subscribe` and `unsubscribe` are the asker's alone.**
   `latest` and `changelog` are too unless `replies = "public"`, or the
   asker asks to `share`. Matrix has no answer one member sees alone, so it
   answers in a thread under the command.

8. **Setting a destination up is a command of its own.**
   `herald discord commands` registers the commands, in one server or in
   all; `herald slack manifest` prints the app's manifest;
   `herald matrix registration` prints the registration with two new
   tokens, once.

## Options considered

| Option | Why it lost |
|--------|-------------|
| **One transport per destination, as ADR-0004 had** | Commands would need a public endpoint everywhere, and Matrix its administrator. A destination that offers a session is served without either. |
| **Asking the source for `latest` when it is asked** | Discord waits three seconds for an answer, a source can be slow or down, and polling it per command spends its rate limit. |
| **Reading `latest` from the ledger** | The ledger keeps steps by key, not the event's text, and has no index by project. |
| **A channel bound to one project in the config** | One channel would answer for one project, where a channel often carries several. |
| **Public answers by default** | A member asking what changed would post into a channel others read; sharing stays one option away. |

## Consequences

**A config's commands run in more places.** The gateway, Socket Mode and a
sync serve a herald behind a firewall; HTTP serves a serverless one.

**Every store but markers keeps one more record.** It is small, read by
commands, and written by every announcement.

**A session is held by `herald server` alone.** A serverless driver serves
the endpoint transports, and doctor says which a layer lacks.

## Related

- [ADR-0002](0002-three-layers-of-support-and-a-doctor-that-names-the-gaps.md): the layers, and what doctor names
- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): the commands and the button, superseded in part here
- [ADR-0005](0005-an-announcement-is-sent-once-and-finished-if-interrupted.md): the store and its subscriptions
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): verifying each request first
- [ADR-0013](0013-a-webhook-delivery-is-admitted-once-released-when-it-fails-and-kept-for-a-week.md): admitting each delivery once
- [ADR-0014](0014-herald-server-answers-first-and-works-after-on-one-task-within-limits.md): answering first and working after
