# ADR-0097: An import reads what it may, maps what has an equivalent, names what has none, and writes secret names only

**Status:** Accepted
**Date:** 2026-11-27

## Context

Teams that adopt herald already announce somewhere: the GitHub app for
Slack or Teams (`/github subscribe`, no API to read a channel's
subscriptions; syntax and defaults documented at
<https://docs.github.com/en/integrations/how-tos/slack/customize-notifications>),
repository and organization webhooks pointing at Discord or Slack
(`GET /repos/{o}/{r}/hooks`, Webhooks read), GitLab integrations and hooks
(`GET /projects/:id/integrations`, `read_api`), Forgejo hooks
(`GET /repos/{o}/{r}/hooks`), and release-announcing Actions in
`.github/workflows`. A chat webhook's URL is its credential. herald never
writes a secret to a file, prints one or takes one from argv (ADR-0007).

## Decision

1. **`herald import <from>`**, each subcommand writing nothing unless
   confirmed, `--print` printing the candidate, `--yes` writing it without
   asking, through the wizard's load-before-write and `toml_edit`
   (ADR-0091):
   - `import github-chat --from slack|teams`: reads pasted
     `/github subscribe …` commands, parsed strictly, or the output of
     `/github subscribe list features`, parsed as far as it can be, each
     line it does not understand shown as "not read"; it asks for the
     channel's id;
   - `import webhooks --forge github|gitlab|forgejo --repo|--org|--group`:
     reads hooks and, on GitLab, integrations, with a read token named by
     `--token-secret NAME` and read from the environment;
   - `import workflows [--dir .github/workflows]`: reads workflow files,
     recognises the known announcing actions by `uses:` at any ref, and
     maps each `${{ secrets.X }}` to `{ secret = "X" }`, so the secret
     keeps its name.
   Each writes sources, destinations, routes and, where one fits, a preset
   (ADR-0092). `--api` takes GitHub Enterprise Server, GHE.com and
   self-managed GitLab and Forgejo.
2. **Every item is told:** "mapped", "mapped partly: …" or "no
   equivalent: …". Releases, tag pushes, pull and merge requests, issues
   (confidential ones to a private route alone), workflow and pipeline
   runs with their name, event, branch and failure filters, labels and
   vulnerability events map; commits, pushes, branches, deployments,
   reviews, comments, discussions, wiki pages, forks, stars, members and a
   run's actor have no equivalent. One route is written per kind and
   channel.
3. **Read-only by default, and nothing secret leaves memory.** A webhook
   URL read from an API is kept only for its kind and host; the import
   names a secret (`HERALD_<DESTINATION>_WEBHOOK`) and says where its value
   is shown (the hook's page in the forge's settings). Every field the
   import does not need, Forgejo's `authorization_header` among them, is
   dropped unread: each answer is read through a closed list of fields.
4. **`--store-secrets` writes a webhook URL into GitHub's Actions secrets**
   (`import webhooks --forge github` only), never to disk or the screen:
   - the URL is read from the hook's config into memory, joins the run's
     scrubber as it is read (ADR-0089), is encrypted with libsodium under
     the repository's public key (`GET …/actions/secrets/public-key`) and
     written with `PUT /repos/{o}/{r}/actions/secrets/{name}`; the output
     names the secret and says it is stored, nothing of its value;
   - the write is made with a GitHub App installation token herald mints
     last, for the one repository, asking `secrets: write`,
     `repository_hooks: read` and `metadata: read`; where GitHub's answer
     names any other permission or repository, the token is revoked at
     once and never used, as ADR-0068 decision 3 does. A personal access
     token is refused: a classic one needs the whole `repo` scope for this
     endpoint (<https://docs.github.com/en/rest/actions/secrets?apiVersion=2022-11-28>),
     and what a fine-grained one holds is not checked by herald, which
     checks only the permissions GitHub names in a mint's answer;
   - a secret of that name that exists is not replaced unless
     `--replace` is given;
   - the App's id and key are secrets named in `--app-secret`, read from
     the environment.
5. **Coexistence.** The import never removes the old setup, and ends with
   "your old setup still posts; switch it off after `herald doctor
   --online --post` passes, or each event posts twice". A route from a
   GitHub chat subscription carries a comment naming where it came from.
6. **Refusals name the permission** an answer lacked, an unknown `uses:`
   is listed unmapped, and a channel name where herald needs an id is
   asked or left to `herald set`.

## Alternatives

| Option | Why not |
|--------|---------|
| **Read-only always** | The user copies each URL from the forge's settings page into a secret by hand; the flag does it without the value ever being shown. |
| **`--store-secrets` on by default** | A token that can write secrets is a different trust from one that reads; it is asked for by name. |
| **Writing the URL into GitLab or Forgejo's secret stores** | Not decided here; the flag is GitHub's. |
| **Remove the old setup** | An import that deletes can leave a team with nothing posting. |

## Consequences

- A token holding `secrets: write` on a repository can create, replace or
  delete every Actions secret of that repository: it could swap a deploy
  credential for one its holder controls, so a workflow sends to another
  account, or delete one to break a deploy. It cannot read any secret's
  value; no endpoint returns one. `repository_hooks: read` reads every
  hook's URL, which for a chat hook is its credential. herald mints such a
  token only under `--store-secrets`, for one repository, and drops it at
  the end of the import.
- An import that maps partly says what it left out.

## Related

- [ADR-0007](0007-secrets-stay-out-of-reach-and-every-inbound-request-is-verified-first.md): secrets
- [ADR-0068](0068-an-operator-signed-in-at-the-forge-re-runs-a-planned-job-once.md): a narrowed App token minted last
- [ADR-0089](0089-herald-s-log-takes-a-level-and-a-format-and-every-secret-herald-resolves-or-sends-is-scrubbed.md): the scrubber
- [ADR-0091](0091-herald-init-writes-a-first-config-and-its-workflow-and-posts-one-test-message.md): the wizard's writing
- [ADR-0092](0092-a-preset-is-shipped-or-local-toml-a-table-adopts-by-name-and-overrides-key-by-key.md): presets
