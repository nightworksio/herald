# ADR-0041: An owner learns of its repositories by push, and reads as a GitHub App

**Status:** Accepted
**Date:** 2026-11-16

## Context

An owner source lists its owner every round (ADR-0029), conditionally, so
a quiet organization costs one request a round. An organization that
already sends herald its webhooks says when a repository joins, leaves or
changes, and needs no listing in between. A GitHub source read with a
personal token or the run's `GITHUB_TOKEN` (ADR-0003): an organization that
installs a GitHub App gets tokens scoped to what it installed, rate limits
that grow with it, and one webhook for every repository it covers.

## Decision

1. **Discovery is a setting.** `discovery = "list"` (the default) lists the
   owner each round. `discovery = "push"` lists it only where the source
   holds no listing, its selection changed (the listing's fingerprint), or
   a delivery said the owner's repositories changed. Membership is always
   rebuilt from a listing, conditional where it can be: a delivery marks
   the listing stale, and the next listing clears the mark, so a payload
   that names a repository only in part (an installation's change names
   no topics or properties) never decides the selection.
2. **A delivery saying the owner's repositories changed relists at once.**
   GitHub's `repository` and `installation_repositories` events and
   Forgejo's `repository` event, of a repository the owner holds, mark the
   owner's listing stale and poll the source, under either discovery.
   herald reads no GitLab event of a project joining a group; doctor says
   so for a GitLab source discovering by push, and for any source
   discovering by push that names no webhook. A layer with no endpoint
   names `discovery = "push"` as a gap.
3. **A GitHub source may read as a GitHub App.** `[github_apps.<name>]`
   names the App by `client_id` or `app_id`, its `private_key` (PKCS#1 as
   GitHub hands it out, or PKCS#8), its `api` (GitHub Enterprise Server and
   GHE.com included) and, where it takes webhooks, its `webhook` secret. A
   source names it with `app`, in place of `token`, and may fix its
   `installation`; otherwise the owner's installation is looked up as an
   organization's, then a user's, and a repository's at the repository.
   The App and its sources reach one host.
4. **herald mints the tokens.** Before each round, each installation the
   sources read gets an RS256 JWT, signed through the host's signer
   (iat a minute back, nine minutes long), exchanged for an installation
   token. The token is kept in memory for the composition and minted again
   five minutes before it expires. A source whose token could not be
   minted is not polled that round, and says why. `store_token = true`
   keeps each token in the store too, so a later run reads it in place of
   minting one: a short-lived credential at rest, encrypted with
   AES-256-GCM under a key HKDF-SHA256 derives from the App's private key
   for that purpose alone, bound to its record's name and its expiry as
   the cipher's associated data, in a record the store seals as it seals
   every record. A record that does not open, whether changed, moved from
   another installation or given a later expiry, or whose token is due, is
   minted again. A token the store does not keep is read with all the same: the
   round says so for each source reading it, naming the store and why, and
   `herald doctor --online` names the App's `store_token` as a gap. Each
   host's signer seals and opens (ring natively, Web Crypto on
   JavaScript hosts), and a store that keeps no records refuses the
   setting.
5. **One endpoint per App.** `/hooks/app/<name>` verifies each delivery
   with the App's secret, admits it once, and leaves it to each source
   reading as the App that takes it: an owner source whose owner holds its
   repository, or the source of that repository. A layer with no endpoint
   names the App's webhook as a gap.
6. **A GitLab owner's members may each send their own webhook.** A
   group's own webhooks need Premium or Ultimate, so doctor says so for a
   GitLab owner that names a webhook. `member_hooks = "off"` (the default)
   leaves the group to its own webhook, or to polling. `"setup"` has
   `herald hooks <source>` register a project webhook on each member, and
   `"round"` has each poll round register one on a member that has none
   too; a member's record keeps that it has one. Each webhook points at
   `<[server] url>/hooks/<source>`, is signed with the source's `webhook`,
   and takes Release and tag events. `herald hooks --url` takes the place
   of `[server] url`, and doctor names the address where neither is given.
   `herald doctor --online` names each member with no webhook to herald
   and, under `"round"`, a token without the `api` scope.

## Consequences

- An organization pushing its webhooks lists itself once, then again only
  when it changes.
- A GitHub App's installation replaces a personal token, on github.com and
  on every enterprise host.
- A GitLab group on the Free tier pushes too, one project webhook a member.
