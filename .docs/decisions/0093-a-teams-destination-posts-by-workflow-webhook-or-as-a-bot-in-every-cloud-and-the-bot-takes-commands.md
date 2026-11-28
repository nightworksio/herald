# ADR-0093: A Teams destination posts by workflow webhook or as a bot, in every cloud, and the bot takes commands

**Status:** Accepted
**Date:** 2026-11-27

## Context

Microsoft Teams retired Office 365 Connectors, connector-based incoming
webhooks among them, in May 2026
(<https://learn.microsoft.com/en-us/microsoftteams/platform/webhooks-and-connectors/how-to/add-incoming-webhook>).
Three ways remain to post:

- **A Workflows webhook** ("When a Teams webhook request is received"):
  a URL carrying a SAS signature, which is itself the credential; it takes
  an Adaptive Card in a message envelope, POST only, with no actionable
  messages (<https://learn.microsoft.com/en-us/connectors/teams/>); at
  most 28 KB a message and 4 requests a second (add-incoming-webhook page).
  Posts appear as the Workflows bot; in GCC, GCC High and DoD the flow
  posts as a user (connectors page); a workflow is orphaned when its owner
  leaves.
- **A Bot Framework bot**, posting proactively once its app is installed
  in the team or chat, never in a private channel
  (<https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/conversations/send-proactive-messages>):
  a client-credentials token, valid 3600 s, for scope
  `https://api.botframework.com/.default`
  (<https://learn.microsoft.com/en-us/azure/bot-service/rest-api/bot-framework-rest-connector-authentication?view=azure-bot-service-4.0>);
  send, reply, update (`PUT`) and delete
  (<https://learn.microsoft.com/en-us/azure/bot-service/rest-api/bot-framework-rest-connector-api-reference?view=azure-bot-service-4.0>);
  thread replies through `replyChains`, in preview
  (<https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/conversations/agent-chat-threads>);
  mentions as `<at>` with an entity by Entra object id or UPN; 7 a second
  per thread, 1800 an hour, 429 with retry on 429, 412, 502, 504
  (<https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/rate-limit>);
  100 KB a message, 413 above.
- **Graph**, where posting as an application is allowed only for
  migration (<https://learn.microsoft.com/en-us/graph/api/channel-post-messages?view=graph-rest-1.0&tabs=http>).

Adaptive Cards in Teams go to version 1.6; their TextBlock Markdown takes
bold, italic, lists and links, and no headings, tables, images,
preformatted text, block quotes or HTML
(<https://learn.microsoft.com/en-us/microsoftteams/platform/task-modules-and-cards/cards/cards-format>).

## Decision

1. **`kind = "teams"`, in `herald-dest-teams`, takes exactly one of
   `webhook` or a bot**, as Slack and Discord do.
2. **The webhook variant.** `webhook = { secret = "HERALD_TEAMS_WEBHOOK" }`
   is the Workflows URL, redacted as `«secret:HERALD_TEAMS_WEBHOOK»`. It
   posts only: no threads, pins, read-back, edits, mentions or
   interactions. Its `Http` reaches the URL's own HTTPS origin alone. It
   takes the URL's SAS signature as its credential; the trigger's
   "specific users" and "tenant" modes, which need an Entra token, are not
   taken.
3. **The bot variant.** `app_id`, `app_password = { secret =
   "HERALD_TEAMS_APP_PASSWORD" }`, `tenant` (`botframework.com` where
   absent) on the table, and a `conversation` per route. It posts, edits
   (`PUT`), mentions subscribers by Entra object id, and replies in a
   thread where the route says `threads = true`, which doctor names as a
   Microsoft preview. It does not pin and does not read back, so the
   markers store names its gap (ADR-0011) and `herald init` picks a
   records store (ADR-0091). 429, 412, 502 and 504 are retried within the
   run's limits (ADR-0028); 413 stops the step, named.
4. **The bot takes commands and buttons.** Commands and the subscribe
   button arrive as Bot Framework activities at
   `/interactions/<destination>`, on `herald server` and the serverless
   hosts (ADR-0016), under `[destinations.<name>.interactions]`. Each
   request is verified before it is parsed: its body is bounded, and its
   bearer JWT is checked against the cloud's Bot Framework metadata and
   keys (RS256, a key endorsed for Teams), the issuer, the audience
   (`app_id`), the expiry and not-before with five minutes' skew, and the
   service URL it names, which must be the activity's own and lie on the
   origin of the bot's service URL, so nothing herald sends a member's
   activity goes elsewhere. The keys are read again within a day. Any
   failure is refused, saying nothing of why. Each activity is admitted
   once by its conversation and id. This is a new row of ADR-0007
   decision 3's inbound table.
5. **`cloud = "public" | "gcc-high" | "dod"`**, `"public"` where
   absent, picks the login host, the Bot Framework hosts and scope, the
   service URL fallback and the OpenID metadata, from tables kept in one
   place with the URL each value was read at. A cloud whose hosts herald
   has not read from Microsoft's documentation is not offered: Microsoft's
   pages give GCC's service URL alone, so `cloud` takes `public`,
   `gcc-high` and `dod`. Each host is also a setting of its own (`login`,
   `scope`, `service`, `openid`, `issuer`, and the tenant a bot signs in
   under), which replaces
   its cloud's: any other cloud, and a fixture server, is reached by
   writing its hosts.
6. **A dialect of its own (extending ADR-0004 decision 2):** card Markdown
   with links as `[text](url)`, a heading written bold, code as plain
   text, `-` bullets, no HTML; measured in bytes against 28 KB for the
   webhook, and for the bot against the 80 KB of its UTF-16 encoding that
   Microsoft says delivers within its approximate 100 KB, less the card's
   envelope. The card holds
   the title as a large bolder TextBlock, the body as a TextBlock and each
   link as an `Action.OpenUrl` (ADR-0019, ADR-0021); its `version` is the
   lowest that holds every element used, never above 1.6. Text from an
   event is escaped for card Markdown, so `<at>`, Markdown and HTML in a
   release body show as text; a mention is made only from stored
   subscribers.
7. **Privacy.** herald cannot read who may read a Teams channel through
   either variant, so a private announcement goes to a Teams route only
   where the route says `private = true`, the operator's word, which doctor
   lists as the operator's and not herald's check. A view's advisories
   need `vouch_unchecked = true` besides, as on every place herald cannot
   check (ADR-0067).
8. **Doctor `--online`.** The bot's token request answers whether its
   credential is taken. A Workflows webhook cannot be asked without
   posting; doctor says so, and `--post` is its check.

## Alternatives

| Option | Why not |
|--------|---------|
| **Workflows webhook only** | No threads, edits, mentions or commands, which every other chat herald posts to offers. |
| **Bot only** | A bot needs an Azure app registration; the webhook is a URL pasted once. |
| **Graph** | Posting as an application there is for migration alone; a person's delegated token is not a service's. |
| **Connector incoming webhooks** | Retired by Microsoft. |
| **Posting commands without the bot's inbound check** | Every inbound request is verified before it is parsed (ADR-0007). |

## Consequences

- Teams is reached the way Slack and Discord are: a URL, or a bot that
  threads, edits, mentions and answers commands.
- A workflow webhook belongs to a person's flow; the docs say it stops
  when its owner leaves.
- In GCC, GCC High and DoD the webhook's posts appear as the flow's owner.

## Related

- [ADR-0004](0004-each-destination-gets-its-own-dialect-limits-and-features.md): dialects; extended here
- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): inbound verification; extended here
- [ADR-0016](0016-commands-are-carried-every-way-a-destination-offers-and-answered-from-what-herald-keeps.md): commands
- [ADR-0067](0067-a-view-shows-its-repositories-advisories-to-private-routes-alone.md): private places
- [ADR-0091](0091-herald-init-writes-a-first-config-and-its-workflow-and-posts-one-test-message.md): the store `herald init` picks
