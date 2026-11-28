# A first run

`herald init` writes a first `herald.toml` (ADR-0091). It asks the forge
and the repository, read from the `origin` remote where it can, the chat
and the way into it, that chat's own questions and the presets to announce
with. Each question is also a flag: `--forge`, `--repository`, `--api`,
`--chat`, `--variant`, `--with <key>=<value>` for each of the chat's own
questions, `--announce` once per preset and `--store-token`. `--yes` takes
the default of every question no flag answers and writes without asking;
`--print` prints the config and writes nothing. It never writes over a
config that exists: `herald setup …` adds to one, and `herald doctor`
judges it.

It names each secret and writes no secret's value. After writing it says,
for each secret, where the forge keeps it and the section below on making
it.

## The test message

Once the config is written, `herald init` shows the test message as
`herald preview` shows a post, and asks whether to post it. It is posted
once, on the config's first route, by `herald doctor --post`'s trial: a
message that says it is a test and announces nothing, kept apart from
every record a real announcement reads.

- Each credential the post needs is read from the environment where its
  secret's variable is set. Otherwise, on a terminal, it is asked for, and
  what is typed is not shown. It is held in memory for that one post and
  never written or printed; everything the post prints is scrubbed of it.
- With `--yes`, where the operator declines, where no terminal is there to
  type a credential on, or where nothing is typed, nothing is posted, and
  it prints the `herald doctor --online --post` command that posts it
  once the environment sets the secret.
- Where the chat refuses the message or cannot be reached, it says what
  the chat answered, scrubbed of the credential, and the secret to check
  and the command to run again; the files stay written, and it exits with
  the doctor's code for a test message that did not land, `1`.

## The store

- A way in that reads its posts back, such as a bot, keeps "sent once" in
  the posts themselves: the `markers` store, which needs nothing.
- A webhook cannot read its posts back. On GitHub herald keeps its records
  in the Actions cache (`actions-cache`), and on Forgejo on the
  repository's `herald-state` branch (`forgejo-branch`), each sealed with
  `HERALD_STORE_KEY`, a key `herald init` makes and prints once.
- No store keeps herald's records in a GitLab repository, so on GitLab a
  first run offers only the ways in that read their posts back.

## Making each credential

### `discord-webhook`

A webhook of the Discord channel herald posts to; its URL is the secret.
Discord's developer documentation describes webhooks at
<https://docs.discord.com/developers/resources/webhook>.

### `discord-bot`

A bot token: create an application in Discord's developer portal; on its
"Bot" page, under "Token", "Reset Token" makes the token, shown once
(<https://docs.discord.com/developers/quick-start/getting-started>). The
bot posts to the channel whose id the first run asks.

### `mastodon-token`

An access token of the account herald posts as, on its server, granted
`write:statuses` and `read:statuses`. Mastodon's documentation describes
obtaining one at <https://docs.joinmastodon.org/client/authorized/>.

### `matrix-bot`

An access token of the bot's account on its homeserver, as a login
answers it (the Matrix specification's "Login" section,
<https://spec.matrix.org/latest/client-server-api/>). The bot posts to a
room it has joined, whose id the first run asks.

### `slack-webhook`

An incoming webhook's URL: create a Slack app, or open one; on its
"Incoming Webhooks" page turn on "Activate Incoming Webhooks", choose "Add
New Webhook to Workspace", pick the channel and "Authorize"; the URL is
listed under "Webhook URLs for Your Workspace"
(<https://docs.slack.dev/messaging/sending-messages-using-incoming-webhooks>).

### `slack-bot`

A bot token (`xoxb-…`) of a Slack app installed in the workspace
(<https://docs.slack.dev/authentication/tokens>). The bot posts to a
channel it has joined, whose id the first run asks.

### `teams-webhook`

A Workflows webhook's URL: in Microsoft Teams, open the channel's "More
options" (…), choose "Workflows" and the template "Send webhook alerts to
a channel", set it up, save it, and copy the webhook link it shows
(<https://learn.microsoft.com/en-us/microsoftteams/platform/webhooks-and-connectors/how-to/add-incoming-webhook>).
Its trigger, "When a Teams webhook request is received", must let anyone
trigger it: herald sends the URL alone, and the trigger's other choices
want a Microsoft Entra token with each request
(<https://learn.microsoft.com/en-us/connectors/teams/>). In Microsoft's
GCC, GCC High and DoD clouds the workflow's post cannot be made as the Flow
bot, so it posts as a user (the same page). The workflow belongs to the
person who made it and is orphaned when they leave unless it has
co-owners (the first page). The URL can be longer than 255 characters.

### `teams-bot`

A bot's app registration and a client secret of it: in the Azure portal,
create an "Azure Bot" resource with a single-tenant app (Microsoft no
longer creates bots registered for every tenant); its "Configuration"
page shows the "Microsoft App ID" and "App Tenant ID" the first run asks,
and "Manage", next to the app id, leads to "Certificates + secrets", where
a new client secret is made and its value shown once
(<https://learn.microsoft.com/en-us/azure/bot-service/bot-service-quickstart-registration?view=azure-bot-service-4.0>).
The secret is the value herald's secret holds. In GCC High and DoD the bot
is registered on Azure Government and `cloud` names it
(<https://learn.microsoft.com/en-us/azure/bot-service/how-to-deploy-gov-cloud-high?view=azure-bot-service-4.0>).

The bot posts only where its app is installed: in the team of the
channel, or in the chat
(<https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/conversations/send-proactive-messages>).
The conversation the first run asks is the channel's or chat's id, which
starts with `19:`. A channel's link holds it, URL-encoded: in
`https://teams.microsoft.com/l/channel/19%3A…%40thread.tacv2/…` the id is
`19:…@thread.tacv2`; a chat message's "Copy link" holds the chat's
(<https://learn.microsoft.com/en-us/microsoftteams/platform/concepts/build-and-test/deep-link-teams>).
A route that says `threads = true` posts each overflow part as a reply in
its post's thread, through an endpoint Microsoft marks as a preview
(<https://learn.microsoft.com/en-us/microsoftteams/platform/bots/how-to/conversations/agent-chat-threads>).

With `[destinations.<name>.interactions]` the bot takes what Teams posts
to it at `https://<your host>/interactions/<name>`, the messaging endpoint
its Azure Bot names, on `herald server` or a serverless host. Each
activity is taken only on the bearer token the Bot Framework vouches for
it with: herald reads the signing keys from its cloud's Bot Framework
metadata document (`openid`) and reads them again within a day, and
checks the token's issuer (`issuer`), its audience (the app id), its times
give or take five minutes, that its key is endorsed for Teams, and that
the service URL it names is the activity's own and lies on the origin of
the bot's `service`
(<https://learn.microsoft.com/en-us/azure/bot-service/rest-api/bot-framework-rest-connector-authentication?view=azure-bot-service-4.0>).
Anything else is refused before its body is read, with an answer that
says nothing of why. Each activity is taken once and answered.

A member gives the bot a command in a message: after its mention in a
channel or a group chat, `@herald latest herald`, and alone in a chat with
the bot, `latest herald`; `commands` names those it answers, all four
where unset. herald acts only for the member the activity names by
Microsoft Entra object id (`from.aadObjectId`), so subscribing changes that
member's own subscription. Its answer replies to the member's message, where
everyone in the conversation reads it, through the service URL the
activity named, with the bot's token; the token goes to that service
origin alone, and the bot signs in at its `login` host alone.

A route that says `mentions = { subscribers = true }` mentions each member
subscribed to the project it announces by their Microsoft Entra object id,
under the label the wording's `mentioned` phrase gives ("subscriber 1",
"subscriber 2" where unworded), as herald keeps no member's name. With
`subscribe = {}` as well, each post carries a Subscribe button
(`Action.Execute`): a press subscribes the member who pressed it, or
unsubscribes one already subscribed, is answered at once with the
wording's `pressed` phrase, and is said in a reply under the post.

### `email-ses`

Amazon SES in one region. Its address `from` must be one SES verified, and
an account still in SES's sandbox sends only to the addresses and domains
it verified, at most 200 messages a day and one a second
(<https://docs.aws.amazon.com/ses/latest/APIReference-V2/API_SendEmail.html>,
<https://docs.aws.amazon.com/ses/latest/APIReference-V2/API_GetAccount.html>,
<https://docs.aws.amazon.com/ses/latest/dg/quotas.html>). The access key
belongs to an IAM user or role whose policy allows `ses:SendEmail` and
`ses:SendRawEmail` on the identity that verified `from`, which is what
`SendEmail` with a whole message needs, and `ses:GetAccount`, which
`herald doctor --online` asks so that it sends nothing
(<https://docs.aws.amazon.com/service-authorization/latest/reference/list_amazonsimpleemailservicev2.html>).
The secrets hold the key's id and its secret access key; a role's
temporary credentials also need `session_token`, such as
`{ secret = "AWS_SESSION_TOKEN" }`.

herald sends each part of an announcement as one message, whole, by
`SendEmail` to the region's endpoint alone, each request signed with AWS
Signature Version 4. `to` and `cc` stand in the message's fields, and
`bcc` beside the message alone; SES takes at most 50 recipients a message,
and a route naming more is refused as the config loads. Open and click
tracking is off for every message, whatever the account or a configuration
set says. `fips = true` reaches the region's FIPS endpoint where AWS lists
one, `dual_stack = true` its endpoint on IPv6 too, and `endpoint` any
other, such as a VPC endpoint's
(<https://docs.aws.amazon.com/general/latest/gr/ses.html>).

### `forgejo-branch`

A token that may push to the repository's `herald-state` branch: an
access token limited to the one repository, with `write:repository`
(<https://forgejo.org/docs/latest/user/token-scope/>). herald's first
write makes the branch in an empty repository; in one that holds other
branches it is made once, with no history, such as with
`git switch --orphan herald-state`, an empty commit and a push
(ADR-0090).
