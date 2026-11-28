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

It names each secret and never asks for its value. After writing it says,
for each secret, where the forge keeps it and the section below on making
it.

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

### `forgejo-branch`

A token that may push to the repository's `herald-state` branch: an
access token limited to the one repository, with `write:repository`
(<https://forgejo.org/docs/latest/user/token-scope/>). herald's first
write makes the branch in an empty repository; in one that holds other
branches it is made once, with no history, such as with
`git switch --orphan herald-state`, an empty commit and a push
(ADR-0090).
